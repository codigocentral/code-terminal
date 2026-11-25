//! Tabs persistentes: o shell vive no daemon do multiplexer e a janela só
//! anexa. Fechar a janela (ou reiniciar o `code-terminal-server`) desanexa;
//! ao abrir de novo, as tabs voltam no mesmo lugar e no mesmo grupo, com o
//! processo vivo e o histórico.
//!
//! Cada painel de uma tab persistente é uma sessão do daemon. A sessão do
//! primeiro painel representa a tab e guarda (em [`PersistentTabMeta`]) o
//! layout, o título, a posição e o grupo; o app manda esses dados ao daemon
//! de tempos em tempos.

use super::AppState;
use crate::session::workspace::{PaneSnapshot, TabSnapshot};
use crate::ui::pane_layout::PaneId;
#[cfg(unix)]
use crate::ui::tabs::TabId;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Intervalo entre as gravações dos dados das tabs no daemon.
#[cfg_attr(not(unix), allow(dead_code))]
const META_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Dados de uma tab persistente guardados no daemon.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersistentTabMeta {
    /// Sessão que representa a tab (a do primeiro painel).
    pub tab: String,
    /// Posição da tab na janela.
    #[serde(default)]
    pub order: usize,
    /// Tab de cima do grupo (a sessão que a representa), se aninhada.
    #[serde(default)]
    pub parent: Option<String>,
    /// Layout, título e grupo recolhido (só na sessão que representa a tab).
    #[serde(default)]
    pub snapshot: Option<TabSnapshot>,
}

impl PersistentTabMeta {
    pub fn parse(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Tab escondida ("Só esconder") que segue viva no daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundSession {
    /// Sessão que representa a tab.
    pub session: String,
    pub title: String,
    pub cwd: String,
}

/// Como fechar uma tab persistente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistentClose {
    /// Encerrar o shell e o que roda nele.
    End,
    /// Só tirar da janela: segue viva em "Sessões em segundo plano".
    Hide,
}

/// O sistema tem tabs persistentes (o daemon usa socket Unix).
pub fn persistence_supported() -> bool {
    cfg!(unix)
}

/// Dá para tornar persistente uma tab que já roda (entregar o PTY ao
/// daemon). Nos outros sistemas só tabs novas podem ser persistentes.
pub fn adoption_supported() -> bool {
    cfg!(target_os = "linux")
}

/// Nome curto e único para uma sessão nova (`t` + base 36 do relógio e um
/// contador, para várias no mesmo milissegundo).
pub fn new_session_name() -> String {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut n = ((nanos / 1_000_000) % 36u128.pow(6)) * 1296 + u128::from(count % 1296);
    let mut digits = Vec::new();
    for _ in 0..8 {
        digits.push(std::char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    digits.reverse();
    format!("t{}", digits.into_iter().collect::<String>())
}

impl AppState {
    /// A tab `index` é persistente (algum painel dela vive no daemon).
    pub fn is_tab_persistent(&self, index: usize) -> bool {
        self.tab_manager.tabs().get(index).is_some_and(|tab| {
            tab.pane_ids()
                .iter()
                .any(|id| self.mux_panes.contains_key(id))
        })
    }

    /// A tab ativa é persistente.
    pub fn is_active_tab_persistent(&self) -> bool {
        self.is_tab_persistent(self.tab_manager.active_tab_index())
    }

    /// Sessão que representa a tab (a do primeiro painel persistente).
    fn tab_session(&self, tab: &crate::ui::tabs::Tab) -> Option<String> {
        tab.pane_ids()
            .iter()
            .find_map(|id| self.mux_panes.get(id).cloned())
    }

    /// Layout, título e grupo da tab, para guardar.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(super) fn tab_snapshot(&self, tab: &crate::ui::tabs::Tab) -> Option<TabSnapshot> {
        let focused = tab.focused_pane();
        let layout = tab.layout().export(|id| PaneSnapshot {
            cwd: tab.pane_buffer(id).and_then(super::buffer_cwd),
            focused: id == focused,
            mux_session: self.mux_panes.get(&id).cloned(),
        })?;
        Some(TabSnapshot {
            layout,
            title: tab.custom_title().map(str::to_string),
            parent: None,
            collapsed: tab.is_collapsed(),
            // Cores escolhidas (#24) ficam no daemon junto com a tab.
            color: tab.color.map(crate::ui::tab_colors::to_hex),
            background: tab.background.map(crate::ui::tab_colors::to_hex),
        })
    }

    /// Dados de cada sessão persistente desta janela (sessão → JSON).
    #[cfg_attr(not(unix), allow(dead_code))]
    fn persistent_metas(&self) -> Vec<(String, String)> {
        let tabs = self.tab_manager.tabs();
        let mut out = Vec::new();
        for (order, tab) in tabs.iter().enumerate() {
            let Some(lead) = self.tab_session(tab) else {
                continue;
            };
            let parent = self
                .tab_manager
                .parent_index(order)
                .and_then(|index| tabs.get(index))
                .and_then(|parent| self.tab_session(parent));
            for id in tab.pane_ids() {
                let Some(session) = self.mux_panes.get(&id) else {
                    continue;
                };
                let meta = PersistentTabMeta {
                    tab: lead.clone(),
                    order,
                    parent: parent.clone(),
                    snapshot: (*session == lead).then(|| self.tab_snapshot(tab)).flatten(),
                };
                out.push((session.clone(), meta.to_json()));
            }
        }
        out
    }

    /// Grava no daemon o que mudou nos dados das tabs persistentes (título,
    /// grupo, posição, layout). Com `force`, grava tudo agora.
    pub fn sync_persistent_meta(&mut self, force: bool) {
        #[cfg(unix)]
        {
            if self.mux_panes.is_empty() {
                return;
            }
            let due = self
                .persistent_synced
                .is_none_or(|at| at.elapsed() >= META_SYNC_INTERVAL);
            if !force && !due {
                return;
            }
            self.persistent_synced = Some(std::time::Instant::now());
            for (session, meta) in self.persistent_metas() {
                let flags = crate::multiplexer::app_client::TabFlags {
                    auto_resume: self.auto_resume_tabs.contains(&session),
                    ..Default::default()
                };
                // O que já foi gravado inclui a marca de retomar sozinha.
                let sent = format!("{}{meta}", u8::from(flags.auto_resume));
                if !force && self.persistent_sent.get(&session) == Some(&sent) {
                    continue;
                }
                match crate::multiplexer::app_client::set_meta(&session, &meta, flags) {
                    Ok(()) => {
                        self.persistent_sent.insert(session, sent);
                    }
                    Err(e) => tracing::debug!(session, error = %e, "Failed to sync tab data"),
                }
            }
        }
        #[cfg(not(unix))]
        let _ = force;
    }

    /// Nova tab persistente (shell novo no daemon, no diretório atual).
    /// Devolve o nome da sessão.
    pub fn open_persistent_tab(&mut self) -> Result<String> {
        let name = new_session_name();
        let cwd = self.focused_cwd();
        let (cols, rows) = self.terminal_size;
        let (pane, buffer) =
            super::spawn_persistent_pane(&self.config, cols, rows, cwd.as_deref(), &name, None)?;
        self.tab_manager.create_tab_with_pane("bash", buffer, pane);
        self.mux_panes.insert(pane, name.clone());
        let last = self.tab_manager.tab_count().saturating_sub(1);
        self.go_to_tab(last);
        self.sync_persistent_meta(true);
        self.refresh_sidebar(true);
        Ok(name)
    }

    /// Liga/desliga a persistência da tab `index`. Devolve como ficou.
    pub fn toggle_tab_persistent(&mut self, index: usize) -> Result<bool> {
        if self.is_tab_persistent(index) {
            self.make_tab_ephemeral(index)?;
            Ok(false)
        } else {
            self.make_tab_persistent(index)?;
            Ok(true)
        }
    }

    /// Liga/desliga a persistência da tab `index` e avisa na janela como ficou.
    pub fn toggle_tab_persistent_notify(&mut self, index: usize) {
        use crate::ui::toast::ToastKind;
        match self.toggle_tab_persistent(index) {
            Ok(true) => self.show_toast(
                crate::ui::i18n::tr(crate::ui::i18n::Text::PersistentOn),
                ToastKind::Info,
            ),
            Ok(false) => self.show_toast(
                crate::ui::i18n::tr(crate::ui::i18n::Text::PersistentOff),
                ToastKind::Info,
            ),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to toggle persistent tab");
                self.show_toast(
                    crate::ui::i18n::persistent_failed(false, &e.to_string()),
                    ToastKind::Error,
                );
            }
        }
    }

    /// A tab `index` retoma o programa sozinha depois de reiniciar a
    /// máquina (senão o comando só fica digitado no prompt).
    pub fn tab_auto_resumes(&self, index: usize) -> bool {
        self.tab_manager.tabs().get(index).is_some_and(|tab| {
            tab.pane_ids().iter().any(|id| {
                self.mux_panes
                    .get(id)
                    .is_some_and(|session| self.auto_resume_tabs.contains(session))
            })
        })
    }

    /// Liga/desliga "retomar automaticamente" na tab persistente `index`.
    pub fn toggle_tab_auto_resume(&mut self, index: usize) -> bool {
        let on = !self.tab_auto_resumes(index);
        let sessions: Vec<String> = self
            .tab_manager
            .tabs()
            .get(index)
            .map(|tab| {
                tab.pane_ids()
                    .iter()
                    .filter_map(|id| self.mux_panes.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default();
        for session in sessions {
            if on {
                self.auto_resume_tabs.insert(session);
            } else {
                self.auto_resume_tabs.remove(&session);
            }
        }
        self.sync_persistent_meta(true);
        self.show_toast(
            crate::ui::i18n::tr(if on {
                crate::ui::i18n::Text::AutoResumeOn
            } else {
                crate::ui::i18n::Text::AutoResumeOff
            }),
            crate::ui::toast::ToastKind::Info,
        );
        on
    }

    /// Painel da sessão persistente `session` (hooks: o id do painel muda a
    /// cada janela, a sessão não).
    pub fn pane_of_mux_session(&self, session: &str) -> Option<PaneId> {
        self.mux_panes
            .iter()
            .find(|(_, name)| name.as_str() == session)
            .map(|(pane, _)| *pane)
    }

    /// Pode alternar a persistência da tab `index` neste sistema.
    pub fn can_toggle_persistent(&self, index: usize) -> bool {
        persistence_supported() && (adoption_supported() || self.is_tab_persistent(index))
    }

    /// Traz de volta todas as tabs escondidas. Devolve quantas voltaram.
    pub fn show_all_background_sessions(&mut self) -> usize {
        use crate::ui::toast::ToastKind;
        let restored = self
            .attach_sessions(|info, _| info.app.as_ref().is_some_and(|app| app.hidden))
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to bring background sessions back");
                0
            });
        let text = crate::ui::i18n::background_sessions_restored(restored);
        self.show_toast(text, ToastKind::Info);
        if restored > 0 {
            let last = self.tab_manager.tab_count().saturating_sub(1);
            self.go_to_tab(last);
        }
        restored
    }

    /// Torna persistente a tab `index`, que já está rodando: cada painel
    /// entrega o PTY ao daemon (o processo não reinicia).
    pub fn make_tab_persistent(&mut self, index: usize) -> Result<()> {
        if !persistence_supported() {
            anyhow::bail!("persistent tabs are not available on this system yet");
        }
        let Some(tab) = self.tab_manager.get_tab_mut(index) else {
            anyhow::bail!("no tab {index}");
        };
        let mut adopted = Vec::new();
        let mut failure = None;
        for id in tab.pane_ids() {
            if self.mux_panes.contains_key(&id) {
                continue;
            }
            let Some(buffer) = tab.pane_buffer_mut(id) else {
                continue;
            };
            if buffer.pty().persistent_session().is_some() {
                // Já é do daemon (deixou de ser persistente antes): volta a ser.
                if let Some(name) = buffer.pty().persistent_session().map(str::to_string) {
                    buffer.pty().set_end_on_drop(false);
                    adopted.push((id, name));
                }
                continue;
            }
            let name = new_session_name();
            let seed = crate::multiplexer::screen::history_to_ansi(buffer.grid());
            match buffer
                .pty_mut()
                .make_persistent(&name, &seed, String::new())
            {
                Ok(()) => adopted.push((id, name)),
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        self.mux_panes.extend(adopted);
        self.sync_persistent_meta(true);
        self.refresh_sidebar(true);
        match failure {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// A tab `index` deixa de ser persistente: continua anexada, mas morre
    /// com a janela, como uma tab comum.
    pub fn make_tab_ephemeral(&mut self, index: usize) -> Result<()> {
        let Some(tab) = self.tab_manager.tabs().get(index) else {
            anyhow::bail!("no tab {index}");
        };
        for id in tab.pane_ids() {
            let Some(session) = self.mux_panes.remove(&id) else {
                continue;
            };
            if let Some(buffer) = tab.pane_buffer(id) {
                buffer.pty().set_end_on_drop(true);
            }
            self.persistent_sent.remove(&session);
            #[cfg(unix)]
            {
                let flags = crate::multiplexer::app_client::TabFlags {
                    kill_on_detach: true,
                    ..Default::default()
                };
                if let Err(e) = crate::multiplexer::app_client::set_meta(&session, "", flags) {
                    tracing::warn!(session, error = %e, "Failed to mark tab as not persistent");
                }
            }
        }
        self.refresh_sidebar(true);
        Ok(())
    }

    /// Fecha uma tab persistente: encerra ou só esconde. Devolve `true` se
    /// a janela ficou sem tabs.
    pub fn close_persistent_tab(&mut self, index: usize, how: PersistentClose) -> bool {
        let Some(tab) = self.tab_manager.tabs().get(index) else {
            return false;
        };
        let lead = self.tab_session(tab);
        for id in tab.pane_ids() {
            let Some(session) = self.mux_panes.get(&id) else {
                continue;
            };
            match how {
                PersistentClose::End => {
                    if let Some(buffer) = tab.pane_buffer(id) {
                        buffer.pty().set_end_on_drop(true);
                    }
                }
                PersistentClose::Hide => {
                    #[cfg(not(unix))]
                    let _ = (session, &lead);
                    #[cfg(unix)]
                    {
                        let meta = PersistentTabMeta {
                            tab: lead.clone().unwrap_or_else(|| session.clone()),
                            order: index,
                            parent: None,
                            snapshot: (Some(session) == lead.as_ref())
                                .then(|| self.tab_snapshot(tab))
                                .flatten(),
                        };
                        let flags = crate::multiplexer::app_client::TabFlags {
                            hidden: true,
                            auto_resume: self.auto_resume_tabs.contains(session),
                            ..Default::default()
                        };
                        if let Err(e) = crate::multiplexer::app_client::set_meta(
                            session,
                            &meta.to_json(),
                            flags,
                        ) {
                            tracing::warn!(session, error = %e, "Failed to hide tab");
                        }
                    }
                }
            }
        }
        // (Os painéis de "Encerrar" já estão marcados; os escondidos só
        // desanexam.)
        self.pending_close_tab = None;
        self.tab_manager.close_tab(index);
        self.forget_closed_panes();
        self.refresh_sidebar(true);
        self.tab_manager.tab_count() == 0
    }

    /// Tira das contas os painéis que fecharam.
    pub(super) fn forget_closed_mux_panes(&mut self) {
        let alive: std::collections::HashSet<PaneId> = self
            .tab_manager
            .tabs()
            .iter()
            .flat_map(|tab| tab.pane_ids())
            .collect();
        let gone: Vec<PaneId> = self
            .mux_panes
            .keys()
            .filter(|id| !alive.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(session) = self.mux_panes.remove(&id) {
                self.persistent_sent.remove(&session);
            }
        }
    }

    /// Tabs escondidas que seguem vivas no daemon.
    pub fn background_sessions(&self) -> Vec<BackgroundSession> {
        #[cfg(unix)]
        {
            let sessions = crate::multiplexer::app_client::list_app_sessions();
            let home = dirs::home_dir();
            sessions
                .iter()
                .filter(|info| matches!(info.state.as_str(), "detached" | "dormant"))
                .filter_map(|info| {
                    let app = info.app.as_ref()?;
                    if !app.hidden || app.kill_on_detach {
                        return None;
                    }
                    let meta = PersistentTabMeta::parse(&app.meta).unwrap_or_default();
                    // Uma entrada por tab: a sessão que a representa.
                    if !meta.tab.is_empty() && meta.tab != info.name {
                        return None;
                    }
                    let title = meta
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.title.clone())
                        .unwrap_or_else(|| info.name.clone());
                    let cwd = crate::ui::sidebar_info::shorten_home(
                        std::path::Path::new(&info.working_dir),
                        home.as_deref(),
                    );
                    Some(BackgroundSession {
                        session: info.name.clone(),
                        title,
                        cwd,
                    })
                })
                .collect()
        }
        #[cfg(not(unix))]
        Vec::new()
    }

    /// Traz de volta uma tab escondida (e os outros painéis dela).
    pub fn show_background_session(&mut self, session: &str) -> Result<()> {
        let restored = self.attach_sessions(|info, meta| {
            info.app.as_ref().is_some_and(|app| app.hidden)
                && (meta.tab == session || info.name == session)
        })?;
        if restored == 0 {
            anyhow::bail!("session {session} is no longer running");
        }
        let last = self.tab_manager.tab_count().saturating_sub(1);
        self.go_to_tab(last);
        Ok(())
    }

    /// Abertura da janela: as tabs persistentes soltas (não escondidas e sem
    /// janela) voltam, na ordem e nos grupos de antes. Devolve quantas.
    pub fn attach_detached_persistent_tabs(&mut self) -> usize {
        // Depois do reboot o daemon ainda não subiu: sobe se há abas gravadas.
        #[cfg(unix)]
        crate::multiplexer::app_client::ensure_daemon_for_saved_tabs();
        match self.attach_sessions(|info, _| info.app.as_ref().is_some_and(|app| !app.hidden)) {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to bring persistent tabs back");
                0
            }
        }
    }

    /// Anexa as sessões soltas que `wanted` aceitar, agrupadas por tab.
    fn attach_sessions(
        &mut self,
        wanted: impl Fn(&crate::multiplexer::SessionInfo, &PersistentTabMeta) -> bool,
    ) -> Result<usize> {
        #[cfg(unix)]
        {
            let sessions = crate::multiplexer::app_client::list_app_sessions();
            let mut candidates: Vec<(crate::multiplexer::SessionInfo, PersistentTabMeta)> =
                sessions
                    .into_iter()
                    // Soltas agora, ou de antes do reboot ("dormant").
                    .filter(|info| matches!(info.state.as_str(), "detached" | "dormant"))
                    .filter(|info| info.app.as_ref().is_some_and(|app| !app.kill_on_detach))
                    .map(|info| {
                        let meta = info
                            .app
                            .as_ref()
                            .and_then(|app| PersistentTabMeta::parse(&app.meta))
                            .unwrap_or_default();
                        (info, meta)
                    })
                    .filter(|(info, meta)| wanted(info, meta))
                    .collect();
            candidates.sort_by_key(|(_, meta)| meta.order);
            let alive: std::collections::HashSet<String> = candidates
                .iter()
                .map(|(info, _)| info.name.clone())
                .collect();
            for (info, _) in &candidates {
                if info.app.as_ref().is_some_and(|app| app.auto_resume) {
                    self.auto_resume_tabs.insert(info.name.clone());
                }
            }
            let dormant: std::collections::HashMap<String, String> = candidates
                .iter()
                .filter(|(info, _)| info.state == "dormant")
                .map(|(info, _)| (info.name.clone(), info.working_dir.clone()))
                .collect();
            let mut plans: Vec<TabPlan> = Vec::new();
            for (info, meta) in &candidates {
                let lead = if meta.tab.is_empty() || !alive.contains(&meta.tab) {
                    info.name.clone()
                } else {
                    meta.tab.clone()
                };
                if let Some(plan) = plans.iter_mut().find(|plan| plan.lead == lead) {
                    if info.name == lead {
                        plan.snapshot = meta.snapshot.clone();
                    }
                    plan.sessions.push(info.name.clone());
                    continue;
                }
                plans.push(TabPlan {
                    lead: lead.clone(),
                    parent: meta.parent.clone(),
                    snapshot: (info.name == lead).then(|| meta.snapshot.clone()).flatten(),
                    sessions: vec![info.name.clone()],
                    dormant_cwd: dormant.get(&lead).cloned(),
                });
            }
            let mut restored = 0;
            let mut placed: Vec<(String, TabId)> = Vec::new();
            for plan in plans {
                match self.attach_tab(&plan) {
                    Ok(id) => {
                        restored += 1;
                        if let Some(parent) = plan
                            .parent
                            .as_ref()
                            .and_then(|parent| placed.iter().find(|(lead, _)| lead == parent))
                            .map(|(_, id)| *id)
                        {
                            let child = self.tab_manager.tab_index_of(id);
                            let parent = self.tab_manager.tab_index_of(parent);
                            if let (Some(child), Some(parent)) = (child, parent) {
                                self.tab_manager.nest_tab(child, parent);
                            }
                        }
                        placed.push((plan.lead.clone(), id));
                    }
                    Err(e) => tracing::warn!(session = %plan.lead, error = %e, "Failed to attach"),
                }
            }
            if restored > 0 {
                let (cols, rows) = self.terminal_size;
                self.resize(cols, rows)?;
                self.sync_persistent_meta(true);
                self.refresh_sidebar(true);
            }
            Ok(restored)
        }
        #[cfg(not(unix))]
        {
            let _ = wanted;
            Ok(0)
        }
    }

    /// Anexa uma tab: o layout guardado (se todos os painéis dele seguem
    /// vivos) ou uma tab por sessão.
    #[cfg(unix)]
    fn attach_tab(&mut self, plan: &TabPlan) -> Result<TabId> {
        let (cols, rows) = self.terminal_size;
        let usable = plan.snapshot.as_ref().filter(|snapshot| {
            let mut all_alive = true;
            let mut count = 0;
            snapshot.layout.for_each(&mut |pane: &PaneSnapshot| {
                count += 1;
                all_alive &= pane
                    .mux_session
                    .as_ref()
                    .is_some_and(|name| plan.sessions.contains(name));
            });
            all_alive && count == plan.sessions.len()
        });
        let (mut tab, panes) = match usable {
            Some(snapshot) => super::restore_tab(&self.config, snapshot, cols, rows)?,
            None => {
                // Viva: anexa. De antes do reboot: o daemon reabre com shell
                // novo e o histórico de antes.
                let (pane, buffer) = super::attach_persistent_pane(
                    &self.config,
                    cols,
                    rows,
                    &plan.lead,
                )
                .or_else(|_| {
                    let cwd = plan.dormant_cwd.as_deref().map(std::path::Path::new);
                    super::spawn_persistent_pane(&self.config, cols, rows, cwd, &plan.lead, None)
                })?;
                let tab = crate::ui::tabs::Tab::with_pane("bash", buffer, pane);
                (tab, vec![(pane, plan.lead.clone())])
            }
        };
        if let Some(snapshot) = usable {
            tab.set_custom_title(snapshot.title.as_deref());
            let hex = |value: &Option<String>| {
                value.as_deref().and_then(crate::ui::tab_colors::parse_hex)
            };
            tab.color = hex(&snapshot.color);
            tab.background = hex(&snapshot.background);
        }
        let id = self.tab_manager.add_tab(tab);
        self.mux_panes.extend(panes);
        if let (Some(snapshot), Some(index)) = (usable, self.tab_manager.tab_index_of(id)) {
            if snapshot.collapsed {
                self.tab_manager.toggle_collapsed(index);
            }
        }
        Ok(id)
    }
}

/// Uma tab a anexar: a sessão que a representa e as dos painéis.
#[derive(Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
struct TabPlan {
    lead: String,
    parent: Option<String>,
    snapshot: Option<TabSnapshot>,
    sessions: Vec<String>,
    /// Diretório de uma aba de antes do reboot.
    dormant_cwd: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_names_are_unique_and_short() {
        let names: std::collections::HashSet<String> =
            (0..500).map(|_| new_session_name()).collect();
        assert_eq!(names.len(), 500);
        assert!(names.iter().all(|n| n.len() == 9 && n.starts_with('t')));
    }

    #[test]
    fn test_meta_round_trips_and_tolerates_garbage() {
        let meta = PersistentTabMeta {
            tab: "tabc".into(),
            order: 3,
            parent: Some("tpai".into()),
            snapshot: None,
        };
        assert_eq!(PersistentTabMeta::parse(&meta.to_json()), Some(meta));
        assert_eq!(PersistentTabMeta::parse("lixo"), None);
        assert_eq!(
            PersistentTabMeta::parse("{\"tab\":\"x\"}").map(|m| m.order),
            Some(0)
        );
    }
}
