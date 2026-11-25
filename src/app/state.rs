use crate::app::assistant::{redact, AssistantTask};
use crate::app::notifications::{send_desktop_notification, NotificationCenter};
use crate::config::Config;
use crate::platform::clipboard::ClipboardManager;
use crate::session::workspace::{PaneSnapshot, TabSnapshot, WorkspaceSnapshot, WORKSPACE_VERSION};
use crate::ssh::QuickConnectParams;
use crate::ssh::SshManager;
use crate::terminal::buffer::TerminalBuffer;
use crate::ui::command_palette::{CommandPalette, PaletteAction};
use crate::ui::mouse_selection::MouseSelectionHandler;
use crate::ui::onboarding::Onboarding;
use crate::ui::overlay_hits::OverlayKind;
use crate::ui::pane_layout::PaneLayout;
use crate::ui::pane_layout::{CellRect, FocusDirection, PaneId, SplitDirection};
use crate::ui::quick_connect::{QuickConnectKey, QuickConnectUI};
use crate::ui::settings::SettingsScreen;
use crate::ui::sidebar_info::{self, TabSidebarInfo};
use crate::ui::tabs::{TabCloseResult, TabId, TabManager};
use anyhow::Result;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{info, trace, warn};

mod persistent;
mod pointer;
mod settings_ui;
mod tab_colors_ui;
pub use settings_ui::SettingsClick;
mod tab_ui;
pub use persistent::{
    adoption_supported, persistence_supported, BackgroundSession, PersistentClose,
    PersistentTabMeta,
};
pub use tab_ui::{SidebarDrag, TabMenu, TabMenuAction, TabMenuEntry, TabRename, TAB_MENU_ROWS};

/// Estado principal da aplicação
pub struct AppState {
    /// Gerenciador de tabs
    pub tab_manager: TabManager,
    /// Configuração do terminal
    pub config: Config,
    /// Dimensões do terminal (cols, rows)
    pub terminal_size: (u16, u16),
    /// Índice da tab pendente de confirmação para fechamento
    pending_close_tab: Option<usize>,
    /// Painel da tab ativa esperando confirmação para fechar (× do painel).
    pending_close_pane: Option<PaneId>,
    /// O que está sob o mouse (botões do cartão e do painel aparecem nele).
    hover: crate::ui::hover::Hover,
    /// Gerenciador de clipboard
    clipboard: Option<ClipboardManager>,
    /// Handler de seleção por mouse
    mouse_selection: MouseSelectionHandler,
    /// Aviso rápido na janela ("Copiado", "Não foi possível colar")
    toast: crate::ui::toast::ToastState,
    /// UI de Quick Connect SSH
    quick_connect_ui: Option<QuickConnectUI>,
    /// O usuário mexeu na seleção da lista do Quick Connect
    quick_connect_navigated: bool,
    /// Command palette
    command_palette: CommandPalette,
    /// Cores de antes de "Choose Theme…": a prévia troca as cores atuais e o
    /// Esc volta a estas. `None` fora da escolha de tema.
    theme_before_preview: Option<crate::config::ColorScheme>,
    /// Tab e alvo da cor sendo digitada em hex (paleta no modo de cor).
    custom_color: Option<(TabId, crate::ui::tab_colors::ColorTarget)>,
    /// Idioma de antes da prévia na tela de configurações (Cancelar volta).
    language_before_preview: Option<crate::ui::i18n::Language>,
    /// Tela de configurações: só existe aberta (o estado some ao fechar).
    settings_screen: Option<Box<SettingsScreen>>,
    /// Onboarding
    onboarding: Onboarding,
    /// Flag indicando que o config foi alterado e precisa ser reaplicado no renderer
    pub config_dirty: bool,
    /// Sidebar de abas verticais visível
    sidebar_visible: bool,
    /// Metadados da sidebar, um por tab (mesma ordem das tabs)
    sidebar_info: Vec<TabSidebarInfo>,
    sidebar_refreshed: Option<std::time::Instant>,
    /// Programa em primeiro plano de cada painel da tab ativa dividida, para
    /// a barra de título (#32). Vazio (sem alocar) quando a tab não é
    /// dividida.
    pane_programs: Vec<(PaneId, String)>,
    pane_programs_refreshed: Option<std::time::Instant>,
    /// `git status` em segundo plano para a sidebar
    git_status: crate::ui::sidebar_info::GitStatusCache,
    /// Memória de cada tab, medida em segundo plano.
    tab_memory: crate::ui::sidebar_info::MemoryCache,
    /// Avisos dos programas (anéis, contador, histórico)
    notifications: NotificationCenter,
    /// Painel de avisos visível
    notifications_panel_visible: bool,
    /// Inspetor de protocolo visível (grava os bytes de todos os painéis)
    inspector_visible: bool,
    /// A janela está em foco (decide se o aviso já foi visto)
    window_focused: bool,
    /// Mandar notificação do desktop (desligado em snapshots e testes)
    pub desktop_notifications: bool,
    /// Assistente de IA: o que o painel mostra e a resposta a caminho
    assistant: Option<AssistantView>,
    assistant_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    assistant_task: Option<AssistantTask>,
    /// Plugins WASM (só na janela principal)
    plugins: Option<crate::app::plugins::PluginHost>,
    /// Título de janela pedido por um plugin, ainda não aplicado
    plugin_title: Option<String>,
    /// Painéis anexados a uma sessão do multiplexer (tab persistente)
    mux_panes: std::collections::HashMap<PaneId, String>,
    /// Dados das tabs persistentes já gravados no daemon (sessão → JSON).
    persistent_sent: std::collections::HashMap<String, String>,
    /// Última gravação dos dados das tabs persistentes.
    #[cfg_attr(not(unix), allow(dead_code))]
    persistent_synced: Option<std::time::Instant>,
    /// Sessões que retomam o programa sozinhas depois do reboot.
    auto_resume_tabs: std::collections::HashSet<String>,
    /// A tab pendente de fechar é persistente: pergunta "Encerrar" ou "Só esconder".
    pending_close_persistent: bool,
    /// Gerenciador SSH
    ssh_manager: Arc<RwLock<SshManager>>,
    /// Sidebar: título em edição, menu de contexto e arraste de tab
    sidebar_ui: tab_ui::SidebarUi,
}

impl AppState {
    /// Cria um novo estado da aplicação
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        Self::new_in(cols, rows, None)
    }

    /// Como [`AppState::new`], com o shell começando em `cwd` (janela pedida
    /// ao modo servidor a partir de outro diretório). Diretório inexistente
    /// é ignorado.
    pub fn new_in(cols: u16, rows: u16, cwd: Option<&std::path::Path>) -> Result<Self> {
        let config = Config::load().unwrap_or_default();
        let cwd = cwd.filter(|dir| dir.is_dir());
        let (pane, buffer) = spawn_pane(&config, cols, rows, None, cwd)?;
        Ok(Self::with_first_buffer(
            config, buffer, pane, "bash", cols, rows,
        ))
    }

    /// Cria o estado com a primeira tab rodando `program` em vez do shell
    /// (usado pelo `--snapshot`).
    pub fn with_command(
        config: Config,
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
    ) -> Result<Self> {
        let (pane, buffer) = spawn_pane(&config, cols, rows, Some((program, args)), None)?;
        Ok(Self::with_first_buffer(
            config, buffer, pane, program, cols, rows,
        ))
    }

    fn with_first_buffer(
        config: Config,
        buffer: TerminalBuffer,
        pane: PaneId,
        title: &str,
        cols: u16,
        rows: u16,
    ) -> Self {
        let mut tab_manager = TabManager::new();
        tab_manager.create_tab_with_pane(title, buffer, pane);

        // Inicializa clipboard (pode falhar em alguns ambientes)
        let clipboard = ClipboardManager::new();
        if clipboard.is_none() {
            warn!("Clipboard not available");
        }

        // Inicializa o gerenciador SSH
        let ssh_manager = Arc::new(RwLock::new(SshManager::new()));

        // Carrega a configuração SSH (se existir)
        if let Err(e) = ssh_manager.read().load_config() {
            warn!("Failed to load SSH config: {}", e);
        }

        let sidebar_visible = config.sidebar.visible;
        crate::ui::i18n::set_language(crate::ui::i18n::language_from_config(&config.language));

        Self {
            tab_manager,
            config,
            terminal_size: (cols, rows),
            pending_close_tab: None,
            pending_close_pane: None,
            hover: Default::default(),
            clipboard,
            mouse_selection: MouseSelectionHandler::new(),
            toast: crate::ui::toast::ToastState::default(),
            quick_connect_ui: None,
            quick_connect_navigated: false,
            command_palette: CommandPalette::new(),
            theme_before_preview: None,
            custom_color: None,
            language_before_preview: None,
            settings_screen: None,
            onboarding: Onboarding::new(),
            config_dirty: false,
            sidebar_visible,
            sidebar_info: Vec::new(),
            sidebar_refreshed: None,
            pane_programs: Vec::new(),
            pane_programs_refreshed: None,
            git_status: Default::default(),
            tab_memory: Default::default(),
            notifications: NotificationCenter::new(),
            notifications_panel_visible: false,
            inspector_visible: false,
            window_focused: true,
            desktop_notifications: !cfg!(test),
            assistant: None,
            assistant_rx: None,
            assistant_task: None,
            plugins: None,
            plugin_title: None,
            mux_panes: std::collections::HashMap::new(),
            persistent_sent: std::collections::HashMap::new(),
            persistent_synced: None,
            auto_resume_tabs: std::collections::HashSet::new(),
            pending_close_persistent: false,
            ssh_manager,
            sidebar_ui: Default::default(),
        }
    }

    // === Sidebar ===

    /// A sidebar de abas verticais está visível?
    pub fn is_sidebar_visible(&self) -> bool {
        self.sidebar_visible
    }

    /// Mostra/esconde a sidebar.
    pub fn toggle_sidebar(&mut self) {
        if self.sidebar_visible {
            // O título em edição fica na sidebar: esconder salva o que foi digitado.
            self.commit_tab_rename();
            self.close_tab_menu();
        }
        self.sidebar_visible = !self.sidebar_visible;
        self.refresh_sidebar(true);
    }

    /// Largura ocupada pela sidebar em pixels (0 se escondida).
    pub fn sidebar_width(&self) -> f32 {
        if self.sidebar_visible {
            self.config.sidebar.width.max(120.0)
        } else {
            0.0
        }
    }

    /// Metadados das tabs (mesma ordem das tabs).
    pub fn sidebar_info(&self) -> &[TabSidebarInfo] {
        &self.sidebar_info
    }

    /// Atualiza diretório, branch e portas de cada tab. Sem `force`, no
    /// máximo a cada 2 s (ler /proc a cada frame seria desperdício).
    pub fn refresh_sidebar(&mut self, force: bool) {
        if !self.sidebar_visible {
            return;
        }
        let fresh = self
            .sidebar_refreshed
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(2));
        if fresh && !force && self.sidebar_info.len() == self.tab_manager.tab_count() {
            return;
        }
        let home = dirs::home_dir();
        let show_memory = self.config.sidebar.show_memory;
        self.sidebar_info = self
            .tab_manager
            .tabs()
            .iter()
            .map(|tab| {
                let unread = self.notifications.unread_for_tab(tab.id);
                let mut info = tab_sidebar_info(tab, home.as_deref(), unread, &self.git_status);
                if show_memory {
                    info.memory_kb = self.tab_memory.get(&tab.pane_pids());
                }
                info.persistent = tab
                    .pane_ids()
                    .iter()
                    .any(|id| self.mux_panes.contains_key(id));
                info
            })
            .collect();
        self.sidebar_refreshed = Some(std::time::Instant::now());
    }

    /// Atualiza o programa de cada painel da tab ativa, no máximo a cada
    /// 2 s (lê `/proc`). Tab sem divisão não guarda nada.
    pub fn refresh_pane_programs(&mut self) {
        let Some(tab) = self.active_tab().filter(|tab| tab.pane_count() > 1) else {
            if !self.pane_programs.is_empty() {
                self.pane_programs = Vec::new();
            }
            self.pane_programs_refreshed = None;
            return;
        };
        let ids = tab.pane_ids();
        let same_panes = self.pane_programs.len() == ids.len()
            && self.pane_programs.iter().all(|(id, _)| ids.contains(id));
        let fresh = self
            .pane_programs_refreshed
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(2));
        if fresh && same_panes {
            return;
        }
        let programs = ids
            .iter()
            .filter_map(|&id| {
                let program = tab.pane_buffer(id)?.pty().foreground_process()?;
                Some((id, program))
            })
            .collect();
        self.pane_programs = programs;
        self.pane_programs_refreshed = Some(std::time::Instant::now());
    }

    /// Texto da barra de título do painel: o programa em primeiro plano e o
    /// título que ele definiu (OSC 0/2) ou o diretório.
    pub fn pane_label(&self, tab: &crate::ui::tabs::Tab, id: PaneId) -> String {
        let program = self
            .pane_programs
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, program)| program.as_str());
        let grid = tab.pane_buffer(id).map(|buffer| buffer.grid());
        let title = grid
            .and_then(|grid| grid.title())
            .filter(|t| !t.trim().is_empty());
        let place = title.map(str::to_string).or_else(|| {
            grid.and_then(|grid| grid.working_dir())
                .map(|dir| crate::ui::sidebar_info::shorten_home(dir, dirs::home_dir().as_deref()))
        });
        match (program, place) {
            (Some(program), Some(place)) => format!("{program} \u{00b7} {place}"),
            (Some(program), None) => program.to_string(),
            (None, Some(place)) => place,
            (None, None) => crate::ui::i18n::tr(crate::ui::i18n::Text::PaneShell).to_string(),
        }
    }

    /// Diretório do painel em foco: OSC 7 do shell primeiro (funciona em
    /// qualquer sistema), senão /proc.
    fn focused_cwd(&self) -> Option<std::path::PathBuf> {
        self.active_tab()
            .and_then(|tab| tab.buffer.as_ref())
            .and_then(|buffer| {
                buffer
                    .grid()
                    .working_dir()
                    .map(std::path::Path::to_path_buf)
                    .or_else(|| buffer.pty().current_dir())
            })
    }

    /// Cria uma nova tab (no diretório em que o usuário está na tab atual).
    pub fn create_new_tab(&mut self) -> Result<()> {
        if self.config.session.persistent_tabs && persistence_supported() {
            match self.open_persistent_tab() {
                Ok(_) => return Ok(()),
                // Sem daemon, a tab abre comum (melhor que não abrir).
                Err(e) => warn!(error = %e, "Persistent tab failed, opening a regular one"),
            }
        }
        let cwd = self.focused_cwd();
        let (cols, rows) = self.terminal_size;
        let (pane, buffer) = spawn_pane(&self.config, cols, rows, None, cwd.as_deref())?;
        self.tab_manager.create_tab_with_pane("bash", buffer, pane);
        let new_index = self.tab_manager.tab_count().saturating_sub(1);
        self.tab_manager.set_active_tab(new_index);

        Ok(())
    }

    // === Painéis (splits) ===

    /// Área do terminal em células (todos os painéis da tab dividem esta área).
    pub fn pane_area(&self) -> CellRect {
        CellRect::new(0, 0, self.terminal_size.0, self.terminal_size.1)
    }

    /// Tamanho do painel que uma divisão criaria na tab ativa.
    fn new_pane_size(&self, direction: SplitDirection) -> (u16, u16) {
        let area = self.pane_area();
        self.active_tab()
            .map(|tab| tab.new_pane_size(direction, area))
            .unwrap_or(self.terminal_size)
    }

    /// Divide o painel em foco; o novo painel abre no mesmo diretório.
    pub fn split_active_pane(&mut self, direction: SplitDirection) -> Result<()> {
        let cwd = self.focused_cwd();
        let (cols, rows) = self.new_pane_size(direction);
        // Painel novo de uma tab persistente também vive no daemon.
        let persistent = self.is_active_tab_persistent();
        let (pane, buffer) = if persistent {
            let name = persistent::new_session_name();
            let spawned =
                spawn_persistent_pane(&self.config, cols, rows, cwd.as_deref(), &name, None)?;
            self.mux_panes.insert(spawned.0, name);
            spawned
        } else {
            spawn_pane(&self.config, cols, rows, None, cwd.as_deref())?
        };
        let area = self.pane_area();
        if let Some(tab) = self.tab_manager.get_active_tab_mut() {
            tab.split_with_id(pane, direction, buffer);
            tab.resize_panes(area);
        }
        Ok(())
    }

    /// Divide o painel em foco rodando `program` no painel novo (snapshots, API).
    pub fn split_active_pane_running(
        &mut self,
        direction: SplitDirection,
        program: &str,
        args: &[String],
    ) -> Result<()> {
        let (cols, rows) = self.new_pane_size(direction);
        let cwd = self.focused_cwd();
        let (pane, buffer) = spawn_pane(
            &self.config,
            cols,
            rows,
            Some((program, args)),
            cwd.as_deref(),
        )?;
        let area = self.pane_area();
        if let Some(tab) = self.tab_manager.get_active_tab_mut() {
            tab.split_with_id(pane, direction, buffer);
            tab.resize_panes(area);
        }
        Ok(())
    }

    /// Abre uma tab rodando `program` (snapshots, API). A tab ativa não muda.
    pub fn open_tab_running(&mut self, program: &str, args: &[String]) -> Result<()> {
        self.open_tab_running_pane(program, args).map(|_| ())
    }

    fn open_tab_running_pane(&mut self, program: &str, args: &[String]) -> Result<PaneId> {
        let (cols, rows) = self.terminal_size;
        let cwd = self.focused_cwd();
        let (pane, buffer) = spawn_pane(
            &self.config,
            cols,
            rows,
            Some((program, args)),
            cwd.as_deref(),
        )?;
        self.tab_manager.create_tab_with_pane(program, buffer, pane);
        Ok(pane)
    }

    /// Link (http/https) na célula do painel em foco, se houver.
    pub fn link_at(&self, row: usize, col: usize) -> Option<String> {
        let grid = self.active_tab().and_then(|tab| tab.grid())?;
        let cells = grid.visible_lines().into_iter().nth(row)?;
        // Uma posição por célula (inclusive as de continuação de caracteres
        // largos), para a coluna do clique bater com a do texto.
        let text: String = cells
            .iter()
            .map(|cell| match cell.char {
                '\0' => ' ',
                _ if cell.spacer => ' ',
                c => c,
            })
            .collect();
        link_in_text(&text, col)
    }

    /// Abre um link onde a configuração manda (janela do navegador do Code
    /// Terminal ou navegador do sistema), sem travar a interface.
    pub fn open_link(&self, url: &str) {
        let url = url.to_string();
        let target = self.config.browser.open_links_in;
        let spawned = std::thread::Builder::new()
            .name("open-link".into())
            .spawn(move || open_link_blocking(&url, target));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "Failed to open link");
        }
    }

    /// Abre a janela do navegador (página em branco, ou a já aberta à frente).
    pub fn open_browser_window(&self) {
        let spawned = std::thread::Builder::new()
            .name("open-browser".into())
            .spawn(|| {
                if crate::browser::remote::request(crate::browser::remote::BrowserRequest::Focus)
                    .is_err()
                {
                    open_link_blocking("about:blank", crate::config::LinkTarget::Builtin);
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "Failed to open browser window");
        }
    }

    /// Sessão do multiplexer em que o painel está anexado.
    pub fn mux_session_of(&self, pane: PaneId) -> Option<&str> {
        self.mux_panes.get(&pane).map(String::as_str)
    }

    /// Fecha o painel em foco. Devolve `true` se a última tab fechou.
    pub fn close_active_pane(&mut self) -> bool {
        match self.active_tab().map(|tab| tab.focused_pane()) {
            Some(id) => self.close_pane(id),
            None => false,
        }
    }

    /// Fecha o painel `id` da tab ativa, sem perguntar (último painel fecha a
    /// tab). Devolve `true` se não sobrou tab.
    pub fn close_pane(&mut self, id: PaneId) -> bool {
        let area = self.pane_area();
        let index = self.tab_manager.active_tab_index();
        let Some(tab) = self.tab_manager.get_active_tab_mut() else {
            return false;
        };
        let Some(buffer) = tab.pane_buffer(id) else {
            return false;
        };
        // Fechar o painel de propósito encerra o shell, também numa tab
        // persistente (senão ele voltaria sozinho na próxima janela).
        if self.mux_panes.remove(&id).is_some() {
            buffer.pty().set_end_on_drop(true);
        }
        if tab.remove_pane(id) {
            self.tab_manager.close_tab(index);
            return self.tab_manager.tab_count() == 0;
        }
        tab.resize_panes(area);
        false
    }

    /// Move o foco para o painel vizinho.
    pub fn focus_pane_direction(&mut self, direction: FocusDirection) -> bool {
        let area = self.pane_area();
        self.tab_manager
            .get_active_tab_mut()
            .is_some_and(|tab| tab.focus_direction(direction, area))
    }

    /// Foca o próximo/anterior painel.
    pub fn cycle_pane(&mut self, forward: bool) -> bool {
        let area = self.pane_area();
        self.tab_manager
            .get_active_tab_mut()
            .is_some_and(|tab| tab.cycle_pane(forward, area))
    }

    /// Aumenta ou diminui o painel em foco.
    pub fn resize_active_pane(&mut self, grow: bool) -> bool {
        let area = self.pane_area();
        let Some(tab) = self.tab_manager.get_active_tab_mut() else {
            return false;
        };
        let changed = tab.resize_focused(grow);
        if changed {
            tab.resize_panes(area);
        }
        changed
    }

    /// Painel na célula (linha, coluna) da área do terminal, com a posição
    /// relativa ao painel. Usado para clique, seleção e mouse.
    pub fn pane_at(&self, row: usize, col: usize) -> Option<(PaneId, usize, usize)> {
        let tab = self.active_tab()?;
        let (id, rect) = tab.layout().content_at(self.pane_area(), row, col)?;
        Some((id, row - rect.row as usize, col - rect.col as usize))
    }

    /// Foca o painel em `id` (clique).
    pub fn focus_pane(&mut self, id: PaneId) -> bool {
        self.tab_manager
            .get_active_tab_mut()
            .is_some_and(|tab| tab.focus_pane(id))
    }

    /// Posição (linha, coluna) relativa ao painel em foco, presa à área dele.
    pub fn focused_pane_cell(&self, row: usize, col: usize) -> (usize, usize) {
        let Some(tab) = self.active_tab() else {
            return (row, col);
        };
        let rect = tab
            .layout()
            .content_rects(self.pane_area())
            .into_iter()
            .find(|(id, _)| *id == tab.focused_pane())
            .map(|(_, rect)| rect);
        match rect {
            Some(rect) if rect.cols > 0 && rect.rows > 0 => (
                row.saturating_sub(rect.row as usize)
                    .min(rect.rows as usize - 1),
                col.saturating_sub(rect.col as usize)
                    .min(rect.cols as usize - 1),
            ),
            _ => (row, col),
        }
    }

    /// Cria uma nova tab executando uma conexão SSH.
    pub fn create_ssh_tab(&mut self, params: &QuickConnectParams) -> Result<()> {
        let args = params.to_ssh_command();
        let buffer = TerminalBuffer::with_command(
            self.terminal_size.0,
            self.terminal_size.1,
            "ssh",
            &args,
            &self.config,
        )?;

        let title = params.connection_string();
        self.tab_manager.create_tab_with_buffer(&title, buffer);
        let new_index = self.tab_manager.tab_count().saturating_sub(1);
        self.tab_manager.set_active_tab(new_index);

        Ok(())
    }

    /// Fecha a tab ativa
    /// Retorna true se a aplicação deve fechar (nenhuma tab restante)
    pub fn close_active_tab(&mut self) -> bool {
        self.close_tab(self.tab_manager.active_tab_index())
    }

    /// Fecha a tab sem pedir confirmação. Devolve `true` se não sobrou tab.
    /// Tab persistente é encerrada (não só desanexada).
    pub fn force_close_tab(&mut self, index: usize) -> bool {
        self.pending_close_tab = None;
        if let Some(tab) = self.tab_manager.tabs().get(index) {
            for id in tab.pane_ids() {
                if let (true, Some(buffer)) =
                    (self.mux_panes.contains_key(&id), tab.pane_buffer(id))
                {
                    buffer.pty().set_end_on_drop(true);
                }
            }
        }
        self.tab_manager.close_tab(index);
        self.forget_closed_panes();
        self.tab_manager.tab_count() == 0
    }

    /// Fecha uma tab específica
    /// Retorna true se a aplicação deve fechar (nenhuma tab restante)
    pub fn close_tab(&mut self, index: usize) -> bool {
        // Tab persistente: sempre pergunta (encerrar ou só esconder).
        if self.is_tab_persistent(index) {
            self.pending_close_tab = Some(index);
            self.pending_close_persistent = true;
            return false;
        }
        self.pending_close_persistent = false;
        match self.tab_manager.try_close_tab(index) {
            TabCloseResult::AllTabsClosed | TabCloseResult::NoTabs => true,
            TabCloseResult::Closed => false,
            TabCloseResult::NeedsConfirmation(_) => {
                // Verifica se a confirmação está habilitada nas configurações
                if self.config.window.confirm_close_tab {
                    // Sinaliza que precisa de confirmação
                    // A UI deve chamar confirm_close_tab() após confirmação do usuário
                    self.pending_close_tab = Some(index);
                    false
                } else {
                    // Confirmação desabilitada, fecha diretamente
                    self.tab_manager.close_tab(index);
                    self.tab_manager.tab_count() == 0
                }
            }
        }
    }

    /// Confirma o fechamento de uma tab que estava pendente de confirmação
    pub fn confirm_close_pending_tab(&mut self) -> bool {
        if let Some(pane) = self.pending_close_pane.take() {
            return self.close_pane(pane);
        }
        let Some(index) = self.pending_close_tab.take() else {
            return false;
        };
        if std::mem::take(&mut self.pending_close_persistent) {
            return self.close_persistent_tab(index, PersistentClose::End);
        }
        self.tab_manager.close_tab(index);
        self.forget_closed_panes();
        self.tab_manager.tab_count() == 0
    }

    /// "Só esconder" a tab persistente pendente de fechar: ela sai da
    /// janela e segue viva no daemon. Devolve `true` se não sobrou tab.
    pub fn hide_pending_close_tab(&mut self) -> bool {
        if !self.pending_close_persistent {
            return false;
        }
        let Some(index) = self.pending_close_tab.take() else {
            return false;
        };
        self.pending_close_persistent = false;
        self.close_persistent_tab(index, PersistentClose::Hide)
    }

    /// A tab pendente de fechar é persistente (o diálogo oferece "Só esconder").
    pub fn pending_close_is_persistent(&self) -> bool {
        self.pending_close_tab.is_some() && self.pending_close_persistent
    }

    /// Cancela o fechamento pendente
    pub fn cancel_close_pending_tab(&mut self) {
        self.pending_close_tab = None;
        self.pending_close_persistent = false;
        self.pending_close_pane = None;
    }

    /// Verifica se há um fechamento (de tab ou de painel) pendente de confirmação
    pub fn has_pending_close_tab(&self) -> bool {
        self.pending_close_tab.is_some() || self.pending_close_pane.is_some()
    }

    /// Retorna o índice da tab pendente de fechamento
    pub fn pending_close_tab_index(&self) -> Option<usize> {
        self.pending_close_tab
    }

    /// Navega para a próxima tab
    pub fn next_tab(&mut self) {
        self.tab_manager.next_tab();
    }

    /// Navega para a tab anterior
    pub fn previous_tab(&mut self) {
        self.tab_manager.previous_tab();
    }

    /// Vai para uma tab específica (0-indexed)
    pub fn go_to_tab(&mut self, index: usize) {
        self.tab_manager.switch_tab(index);
    }

    /// Redimensiona todas as tabs
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.terminal_size = (cols, rows);
        let area = self.pane_area();
        for i in 0..self.tab_manager.tab_count() {
            if let Some(tab) = self.tab_manager.get_tab_mut(i) {
                tab.resize_panes(area);
            }
        }
        Ok(())
    }

    /// Processa output de todas as tabs
    pub fn process_output(&mut self) {
        for i in 0..self.tab_manager.tab_count() {
            if let Some(tab) = self.tab_manager.get_tab_mut(i) {
                for buffer in tab.pane_buffers_mut() {
                    buffer.process_output();
                }
            }
        }
        self.collect_notifications();
        self.poll_assistant();
        self.run_plugins();
        self.sync_persistent_meta(false);
    }

    // === Plugins WASM ===

    /// Carrega os plugins de `dir`. Com algum plugin, um relógio acorda o
    /// app a cada [`UPDATE_INTERVAL`](crate::app::plugins::UPDATE_INTERVAL)
    /// para o `update` deles. Devolve quantos carregaram.
    pub fn enable_plugins(&mut self, dir: &std::path::Path, data_root: &std::path::Path) -> usize {
        let host = match crate::app::plugins::PluginHost::load_dir(dir, data_root) {
            Ok(host) => host,
            Err(error) => {
                tracing::warn!(error = %error, "Plugin runtime unavailable");
                return 0;
            }
        };
        let count = host.names().len();
        if count > 0 {
            start_plugin_clock();
        }
        self.plugins = Some(host);
        count
    }

    /// Nomes dos plugins carregados.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.as_ref().map(|p| p.names()).unwrap_or_default()
    }

    /// Status que os plugins pediram para mostrar (nome, texto).
    pub fn plugin_statuses(&self) -> Vec<(String, String)> {
        self.plugins
            .as_ref()
            .map(|p| p.statuses())
            .unwrap_or_default()
    }

    /// Repassa texto digitado aos plugins; `true` se um deles consumiu (e
    /// então o texto não vai ao shell).
    pub fn plugin_input(&mut self, text: &str) -> bool {
        match self.plugins.as_ref() {
            Some(host) if !host.is_empty() => {
                self.publish_plugin_view();
                host.on_input(text)
            }
            _ => false,
        }
    }

    /// Título de janela pedido por um plugin (uma vez).
    pub fn take_plugin_title(&mut self) -> Option<String> {
        self.plugin_title.take()
    }

    fn run_plugins(&mut self) {
        let Some(host) = self.plugins.as_mut() else {
            return;
        };
        if host.is_empty() {
            return;
        }
        if host.tick() {
            self.publish_plugin_view();
        }
        for request in crate::plugin::bridge::drain() {
            self.apply_plugin_request(request);
        }
    }

    /// Publica o painel em foco para as host functions dos plugins.
    fn publish_plugin_view(&self) {
        let Some(tab) = self.tab_manager.get_active_tab() else {
            return;
        };
        let Some(grid) = tab.grid() else {
            return;
        };
        let (cursor_row, cursor_col) = grid.cursor_position();
        let to_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
        crate::plugin::bridge::publish_view(crate::plugin::bridge::HostView {
            cols: to_u16(grid.cols()),
            rows: to_u16(grid.rows()),
            cursor_col: to_u16(cursor_col),
            cursor_row: to_u16(cursor_row),
            lines: grid
                .visible_lines()
                .iter()
                .map(|row| row_text(row))
                .collect(),
            tab_id: u64::from(tab.id),
            pane_id: tab.focused_pane(),
            selection: grid.get_selected_text(),
            theme: self.config.colors.name.clone(),
        });
    }

    fn apply_plugin_request(&mut self, request: crate::plugin::bridge::PluginRequest) {
        use crate::plugin::bridge::HostCommand;
        let display = match request.command {
            HostCommand::Write(text) => Some(text),
            HostCommand::WriteAt { col, row, text } => {
                Some(crate::app::plugins::write_at_sequence(col, row, &text))
            }
            HostCommand::SendInput(text) => {
                if let Err(error) = self.write_to_active_tab(text.as_bytes()) {
                    tracing::warn!(plugin = %request.plugin, error = %error, "Plugin input failed");
                }
                None
            }
            HostCommand::Notify { message, .. } => {
                if let Some(pane) = self.active_tab().map(|tab| tab.focused_pane()) {
                    self.notify_pane(pane, &request.plugin, &message);
                }
                None
            }
            HostCommand::Status(text) => {
                if let Some(host) = self.plugins.as_mut() {
                    host.set_status(&request.plugin, &text);
                }
                None
            }
            HostCommand::SetTitle(title) => {
                self.plugin_title = Some(title);
                None
            }
        };
        if let Some(text) = display {
            if let Some(buffer) = self
                .tab_manager
                .get_active_tab_mut()
                .and_then(|t| t.buffer.as_mut())
            {
                buffer.inject_output(text.as_bytes());
            }
        }
    }

    // === Avisos (OSC 9/99/777 e campainha) ===

    /// Lê os avisos dos painéis. O que o usuário não está vendo acende o
    /// anel do painel, conta na sidebar e (se tiver texto) vira notificação
    /// do desktop.
    fn collect_notifications(&mut self) {
        let active = self.tab_manager.active_tab_index();
        let focused = self.window_focused;
        let mut pending = Vec::new();
        for index in 0..self.tab_manager.tab_count() {
            if let Some(tab) = self.tab_manager.get_tab_mut(index) {
                let visible = focused && index == active;
                pending.extend(drain_tab_notifications(tab, visible));
            }
        }
        for item in pending {
            self.deliver(item);
        }
    }

    /// Registra um aviso e, se o usuário não está vendo, avisa o desktop.
    fn deliver(&mut self, item: PendingNotification) {
        if !item.seen && self.desktop_notifications {
            if let Some(note) = &item.note {
                let heading = if note.title.is_empty() {
                    &item.tab_title
                } else {
                    &note.title
                };
                send_desktop_notification(heading, &note.body);
            }
        }
        // Só os estilos que se mexem acordam o render (#26).
        let animate = !item.seen && self.motion().style.moves();
        self.notifications
            .record(item.tab_id, item.pane, item.note, item.seen);
        if animate {
            crate::ui::glow::keep_awake(crate::ui::glow::GLOW_DURATION);
        }
    }

    /// Animação da tab com aviso, já resolvida (estilo, velocidade e
    /// "menos movimento" do sistema).
    pub fn motion(&self) -> crate::ui::tab_animation::Motion {
        let window = &self.config.window;
        crate::ui::tab_animation::Motion::resolve(
            window.animations,
            window.tab_animation,
            window.animation_speed,
            window.follow_reduced_motion,
            window.follow_reduced_motion
                && crate::ui::tab_animation::system_prefers_reduced_motion(),
        )
    }

    /// Informa a todos os painéis o tamanho da célula (para medir imagens).
    pub fn set_cell_pixel_size(&mut self, width: f32, height: f32) {
        for index in 0..self.tab_manager.tab_count() {
            if let Some(tab) = self.tab_manager.get_tab_mut(index) {
                for buffer in tab.pane_buffers_mut() {
                    buffer.grid_mut().set_cell_pixel_size(width, height);
                }
            }
        }
    }

    // === Assistente de IA ===

    /// O que o painel do assistente mostra (se aberto).
    pub fn assistant_view(&self) -> Option<&AssistantView> {
        self.assistant.as_ref()
    }

    /// Pede à IA para explicar a saída recente do painel em foco.
    pub fn ai_explain_output(&mut self) {
        let Some(grid) = self.active_tab_grid() else {
            return;
        };
        let text = grid_to_string(grid);
        let lines: Vec<&str> = text.lines().collect();
        let output = lines[lines.len().saturating_sub(80)..].join("\n");
        let home = dirs::home_dir().map(|h| h.display().to_string());
        let output = redact(&output, home.as_deref());
        self.start_assistant(AssistantTask::Explain { output });
    }

    /// Abre o campo para descrever o comando desejado.
    pub fn ai_open_suggest(&mut self) {
        self.assistant = Some(AssistantView::Prompt(String::new()));
    }

    /// Tecla com o painel do assistente aberto. Devolve `true` se consumiu.
    pub fn assistant_key(&mut self, key: AssistantKey) -> bool {
        let Some(view) = self.assistant.as_mut() else {
            return false;
        };
        match (view, key) {
            (_, AssistantKey::Escape) => self.close_assistant(),
            (AssistantView::Prompt(text), AssistantKey::Char(c)) => text.push(c),
            (AssistantView::Prompt(text), AssistantKey::Backspace) => {
                text.pop();
            }
            (AssistantView::Prompt(text), AssistantKey::Enter) => {
                let request = text.trim().to_string();
                if !request.is_empty() {
                    let cwd = self
                        .focused_cwd()
                        .map(|dir| sidebar_info::shorten_home(&dir, dirs::home_dir().as_deref()));
                    let shell = std::env::var("SHELL")
                        .ok()
                        .and_then(|s| s.rsplit('/').next().map(str::to_string))
                        .unwrap_or_else(|| "bash".to_string());
                    self.start_assistant(AssistantTask::Suggest {
                        request,
                        cwd,
                        shell,
                    });
                }
            }
            (AssistantView::Answer { command, .. }, AssistantKey::Enter) => {
                // Insere o comando sem executar: o usuário revisa e dá Enter.
                if let Some(command) = command.clone() {
                    let _ = self.write_to_active_tab(command.as_bytes());
                }
                self.close_assistant();
            }
            (AssistantView::Error(_), AssistantKey::Enter) => self.close_assistant(),
            _ => {}
        }
        true
    }

    fn close_assistant(&mut self) {
        self.assistant = None;
        self.assistant_rx = None;
        self.assistant_task = None;
    }

    /// Confere a configuração e manda o pedido numa thread de fundo.
    fn start_assistant(&mut self, task: AssistantTask) {
        let ai = &self.config.ai;
        if !ai.enabled {
            self.assistant = Some(AssistantView::Error(
                "AI is off. In ~/.config/code-terminal/config.toml set [ai] enabled = true \
                 and provider = \"anthropic\" (or openai/ollama), then export the API key."
                    .to_string(),
            ));
            return;
        }
        let key = crate::app::assistant::api_key(ai);
        let request = match crate::app::assistant::build_request(ai, &task, key.as_deref()) {
            Ok(request) => request,
            Err(e) => {
                let var = if ai.model.api_key_env.is_empty() {
                    match ai.provider {
                        crate::config::ai::AIProvider::Anthropic => "ANTHROPIC_API_KEY",
                        _ => "OPENAI_API_KEY",
                    }
                } else {
                    ai.model.api_key_env.as_str()
                };
                self.assistant = Some(AssistantView::Error(format!(
                    "AI request not sent: {e}. Export {var} before starting Code Terminal."
                )));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("assistant".into())
            .spawn(move || {
                let _ = tx.send(crate::app::assistant::send(&request));
                crate::terminal::pty::request_redraw();
            });
        if let Err(e) = spawned {
            self.assistant = Some(AssistantView::Error(format!("AI request not sent: {e}")));
            return;
        }
        self.assistant = Some(AssistantView::Waiting(task.title()));
        self.assistant_rx = Some(rx);
        self.assistant_task = Some(task);
    }

    /// Recebe a resposta, se já chegou.
    pub fn poll_assistant(&mut self) {
        let Some(rx) = &self.assistant_rx else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.assistant_rx = None;
        let task = self.assistant_task.take();
        self.assistant = Some(match result {
            Ok(text) => {
                let command = match &task {
                    Some(AssistantTask::Suggest { .. }) => {
                        crate::app::assistant::extract_command(&text)
                    }
                    _ => None,
                };
                AssistantView::Answer {
                    title: task.as_ref().map_or("AI", AssistantTask::title),
                    text,
                    command,
                }
            }
            Err(e) => AssistantView::Error(format!("AI request failed: {e}")),
        });
    }

    // === Workspace (sessão) ===

    /// Tabs, painéis (com proporções) e diretórios atuais.
    pub fn capture_workspace(&self) -> WorkspaceSnapshot {
        // Tab sem painel exportável fica de fora; o grupo aponta para a
        // posição na lista salva, não no vetor de tabs.
        let kept: Vec<(TabId, TabSnapshot)> = self
            .tab_manager
            .tabs()
            .iter()
            .filter_map(|tab| {
                let focused = tab.focused_pane();
                let layout = tab.layout().export(|id| PaneSnapshot {
                    cwd: tab.pane_buffer(id).and_then(buffer_cwd),
                    focused: id == focused,
                    mux_session: self.mux_panes.get(&id).cloned(),
                })?;
                let snapshot = TabSnapshot {
                    layout,
                    title: tab.custom_title().map(str::to_string),
                    parent: None,
                    collapsed: tab.is_collapsed(),
                    color: tab.color.map(crate::ui::tab_colors::to_hex),
                    background: tab.background.map(crate::ui::tab_colors::to_hex),
                };
                Some((tab.id, snapshot))
            })
            .collect();
        let ids: Vec<TabId> = kept.iter().map(|(id, _)| *id).collect();
        let tabs = kept
            .into_iter()
            .map(|(id, mut snapshot)| {
                snapshot.parent = self
                    .tab_manager
                    .tab_index_of(id)
                    .and_then(|index| self.tab_manager.tabs()[index].parent())
                    .and_then(|parent| ids.iter().position(|&kept| kept == parent));
                snapshot
            })
            .collect();
        WorkspaceSnapshot {
            version: WORKSPACE_VERSION,
            active_tab: self.tab_manager.active_tab_index(),
            sidebar_visible: self.sidebar_visible,
            tabs,
        }
    }

    /// Troca as tabs atuais pelas do snapshot (cada painel abre um shell no
    /// diretório salvo, se ele ainda existir).
    pub fn restore_workspace(&mut self, snapshot: &WorkspaceSnapshot) -> Result<()> {
        if snapshot.tabs.is_empty() {
            return Ok(());
        }
        let (cols, rows) = self.terminal_size;
        let mut restored = Vec::new();
        let mut mux_sessions = Vec::new();
        for tab_snapshot in &snapshot.tabs {
            let (mut tab, sessions) = restore_tab(&self.config, tab_snapshot, cols, rows)?;
            tab.set_custom_title(tab_snapshot.title.as_deref());
            let hex = |value: &Option<String>| {
                value.as_deref().and_then(crate::ui::tab_colors::parse_hex)
            };
            tab.color = hex(&tab_snapshot.color);
            tab.background = hex(&tab_snapshot.background);
            restored.push(tab);
            mux_sessions.extend(sessions);
        }

        let original = self.tab_manager.tab_count();
        for tab in restored {
            self.tab_manager.add_tab(tab);
        }
        self.mux_panes.extend(mux_sessions);
        for _ in 0..original {
            self.tab_manager.close_tab(0);
        }
        let groups: Vec<(Option<usize>, bool)> = snapshot
            .tabs
            .iter()
            .map(|tab| (tab.parent, tab.collapsed))
            .collect();
        self.tab_manager.restore_groups(&groups);
        let last = self.tab_manager.tab_count().saturating_sub(1);
        self.go_to_tab(snapshot.active_tab.min(last));
        self.sidebar_visible = snapshot.sidebar_visible;
        self.resize(cols, rows)?;
        self.refresh_sidebar(true);
        Ok(())
    }

    /// Aviso vindo de fora (Socket API/CLI) para um painel.
    pub fn notify_pane(&mut self, pane: PaneId, title: &str, body: &str) {
        let active = self.tab_manager.active_tab_index();
        let Some((index, tab)) = self
            .tab_manager
            .tabs()
            .iter()
            .enumerate()
            .find(|(_, tab)| tab.pane_ids().contains(&pane))
        else {
            return;
        };
        let seen = self.window_focused && index == active && tab.focused_pane() == pane;
        let (tab_id, tab_title) = (tab.id, tab.title());
        if !seen && self.desktop_notifications {
            let heading = if title.is_empty() {
                tab_title.as_str()
            } else {
                title
            };
            send_desktop_notification(heading, body);
        }
        let note = crate::terminal::grid::TerminalNotification {
            title: title.to_string(),
            body: body.to_string(),
        };
        self.notifications.record(tab_id, pane, Some(note), seen);
    }

    /// Informa se a janela está em foco; ao voltar, o painel visível é lido.
    pub fn set_window_focused(&mut self, focused: bool) {
        self.window_focused = focused;
        self.mark_visible_seen();
    }

    /// O painel que o usuário está vendo deixa de ter aviso não lido.
    pub fn mark_visible_seen(&mut self) {
        if !self.window_focused {
            return;
        }
        if let Some(pane) = self.active_tab().map(|tab| tab.focused_pane()) {
            self.notifications.mark_seen(pane);
        }
    }

    /// Central de avisos (anéis, contadores e histórico).
    pub fn notifications(&self) -> &NotificationCenter {
        &self.notifications
    }

    /// Vai para o painel com o aviso não lido mais recente.
    pub fn jump_to_unread(&mut self) -> bool {
        let Some((tab_id, pane)) = self.notifications.latest_unread() else {
            return false;
        };
        let Some(index) = self.tab_manager.tabs().iter().position(|t| t.id == tab_id) else {
            self.notifications.forget_pane(pane);
            return false;
        };
        self.go_to_tab(index);
        self.focus_pane(pane);
        self.notifications.mark_seen(pane);
        true
    }

    /// Liga/desliga o inspetor de protocolo (grava os bytes de cada painel).
    pub fn toggle_inspector(&mut self) {
        self.inspector_visible = !self.inspector_visible;
        let enabled = self.inspector_visible;
        for index in 0..self.tab_manager.tab_count() {
            if let Some(tab) = self.tab_manager.get_tab_mut(index) {
                for buffer in tab.pane_buffers_mut() {
                    buffer.set_inspector(enabled);
                }
            }
        }
    }

    pub fn is_inspector_visible(&self) -> bool {
        self.inspector_visible
    }

    /// Mostra/esconde o painel com o histórico de avisos.
    pub fn toggle_notifications_panel(&mut self) {
        self.notifications_panel_visible = !self.notifications_panel_visible;
    }

    pub fn is_notifications_panel_visible(&self) -> bool {
        self.notifications_panel_visible
    }

    /// Esquece avisos de painéis que fecharam.
    fn forget_closed_panes(&mut self) {
        let alive: std::collections::HashSet<PaneId> = self
            .tab_manager
            .tabs()
            .iter()
            .flat_map(|tab| tab.pane_ids())
            .collect();
        self.notifications
            .retain_panes(|pane| alive.contains(&pane));
        self.forget_closed_mux_panes();
    }

    /// Processa output apenas da tab ativa (mais eficiente)
    pub fn process_active_tab_output(&mut self) {
        if let Some(tab) = self.tab_manager.get_active_tab_mut() {
            if let Some(ref mut buffer) = tab.buffer {
                buffer.process_output();
            }
        }
    }

    /// Fecha as tabs cujo processo (shell) já terminou, como `exit` ou Ctrl+D.
    /// Retorna `true` se não sobrou nenhuma tab (a janela deve fechar).
    pub fn close_exited_tabs(&mut self) -> bool {
        let area = self.pane_area();
        let mut closed_any = false;
        let mut index = 0;
        while index < self.tab_manager.tab_count() {
            let Some(tab) = self.tab_manager.get_tab_mut(index) else {
                break;
            };
            let had_panes = tab.pane_count();
            let empty = had_panes > 0 && tab.remove_exited_panes();
            if empty {
                info!(tab_index = index, "Shell exited, closing tab");
                self.tab_manager.close_tab(index);
                closed_any = true;
                continue;
            }
            if tab.pane_count() != had_panes {
                // Um painel fechou: os vizinhos ocupam o espaço.
                tab.resize_panes(area);
            }
            index += 1;
        }
        self.forget_closed_panes();
        closed_any && self.tab_manager.tab_count() == 0
    }

    /// Retorna a tab ativa
    pub fn active_tab(&self) -> Option<&crate::ui::tabs::Tab> {
        self.tab_manager.get_active_tab()
    }

    /// Retorna a tab ativa (mutável)
    pub fn active_tab_mut(&mut self) -> Option<&mut crate::ui::tabs::Tab> {
        self.tab_manager.get_active_tab_mut()
    }

    /// Escreve dados na tab ativa
    pub fn write_to_active_tab(&mut self, data: &[u8]) -> Result<()> {
        if let Some(tab) = self.tab_manager.get_active_tab_mut() {
            if let Some(ref mut buffer) = tab.buffer {
                buffer.write(data)?;
            }
        }
        Ok(())
    }

    /// Retorna o grid da tab ativa (para renderização)
    pub fn active_tab_grid(&self) -> Option<&crate::terminal::grid::Grid> {
        self.tab_manager.get_active_tab().and_then(|tab| tab.grid())
    }

    /// Retorna o número de tabs
    pub fn tab_count(&self) -> usize {
        self.tab_manager.tab_count()
    }

    /// Retorna o índice da tab ativa
    pub fn active_tab_index(&self) -> usize {
        self.tab_manager.active_tab_index()
    }

    /// Verifica se o clipboard está disponível
    pub fn clipboard_available(&self) -> bool {
        self.clipboard.is_some()
    }

    /// Copia o conteúdo visível da tab ativa para o clipboard
    ///
    /// Por enquanto copia todo o conteúdo visível.
    /// Futuramente: copiar apenas a seleção quando implementado.
    pub fn copy_visible_content(&self) -> Result<()> {
        let clipboard = self
            .clipboard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Clipboard not available"))?;

        let content = if let Some(grid) = self.active_tab_grid() {
            grid_to_string(grid)
        } else {
            String::new()
        };

        if content.is_empty() {
            trace!("No content to copy");
            return Ok(());
        }

        clipboard
            .copy_text(&content)
            .map_err(|e| anyhow::anyhow!("Failed to copy to clipboard: {}", e))?;

        trace!("Copied {} characters to clipboard", content.len());
        Ok(())
    }

    /// Cola o conteúdo do clipboard na tab ativa
    pub fn paste_from_clipboard(&mut self) -> Result<()> {
        let clipboard = self
            .clipboard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Clipboard not available"))?;

        let content = match clipboard.get_text() {
            Ok(content) => content,
            Err(crate::platform::clipboard::ClipboardError::EmptyClipboard) => {
                return Err(NothingToPaste.into())
            }
            Err(e) => return Err(anyhow::anyhow!("Failed to read from clipboard: {}", e)),
        };

        self.paste_text(&content)
    }

    /// Cola a seleção PRIMARY (clique do meio no Linux). Em outros sistemas,
    /// cola a área de transferência.
    pub fn paste_primary(&mut self) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let clipboard = self
                .clipboard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Clipboard not available"))?;
            let content = match clipboard.get_primary_text() {
                Ok(content) => content,
                Err(crate::platform::clipboard::ClipboardError::EmptyClipboard) => {
                    return Err(NothingToPaste.into())
                }
                Err(e) => return Err(anyhow::anyhow!("Failed to read PRIMARY selection: {}", e)),
            };
            self.paste_text(&content)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.paste_from_clipboard()
        }
    }

    /// Escreve `content` na tab ativa como colagem.
    fn paste_text(&mut self, content: &str) -> Result<()> {
        if content.is_empty() {
            trace!("No content to paste");
            return Ok(());
        }

        // Quebras viram \r; com bracketed paste o shell recebe o bloco inteiro
        // sem executar linha a linha.
        let bracketed = self
            .active_tab_grid()
            .is_some_and(|grid| grid.modes().bracketed_paste);
        let bytes = crate::ui::key_encoding::paste_bytes(content, bracketed);

        self.write_to_active_tab(&bytes)?;

        trace!("Pasted {} characters from clipboard", content.len());
        Ok(())
    }

    /// Cola (área de transferência, ou PRIMARY com `primary`) e avisa na
    /// janela se não der certo.
    pub fn paste_and_notify(&mut self, primary: bool) {
        let result = if primary {
            self.paste_primary()
        } else {
            self.paste_from_clipboard()
        };
        if let Err(e) = result {
            let message = if e.downcast_ref::<NothingToPaste>().is_some() {
                tracing::debug!("Nothing to paste: {}", e);
                crate::ui::toast::nothing_to_paste()
            } else {
                tracing::warn!("Failed to paste: {}", e);
                crate::ui::toast::paste_failed()
            };
            self.show_toast(message, crate::ui::toast::ToastKind::Error);
        }
    }

    /// Mostra um aviso rápido e agenda um redesenho para quando ele sumir
    /// (o event loop dorme até chegar um evento).
    pub fn show_toast(&mut self, text: impl Into<String>, kind: crate::ui::toast::ToastKind) {
        self.toast.show(text, kind, std::time::Instant::now());
        let wake_after = crate::ui::toast::TOAST_DURATION + std::time::Duration::from_millis(50);
        let spawned = std::thread::Builder::new()
            .name("toast-clock".into())
            .spawn(move || {
                std::thread::sleep(wake_after);
                crate::terminal::pty::request_redraw();
            });
        if let Err(error) = spawned {
            tracing::warn!(error = %error, "Failed to schedule toast redraw");
        }
    }

    /// O aviso visível agora, se houver.
    pub fn toast(&self) -> Option<&crate::ui::toast::Toast> {
        self.toast.visible(std::time::Instant::now())
    }

    /// Texto da seleção atual que vale copiar ao soltar o mouse: vazio
    /// quando não há seleção ou quando é só o clique (início = fim).
    fn selection_to_copy(&self) -> String {
        self.active_tab_grid()
            .filter(|grid| selection_worth_copying(grid.selection()))
            .map(|grid| grid.get_selected_text())
            .unwrap_or_default()
    }

    /// Copiar ao selecionar: chamado ao soltar o botão esquerdo. Copia a
    /// seleção (área de transferência e PRIMARY) e avisa na janela.
    /// Devolve quantos caracteres copiou.
    pub fn copy_on_select(&mut self) -> Option<usize> {
        if !self.config.terminal.copy_on_select {
            return None;
        }
        let text = self.selection_to_copy();
        self.copy_and_notify(&text)
    }

    /// Copia a seleção pelo atalho (Ctrl+Shift+C) e avisa na janela.
    pub fn copy_selection_and_notify(&mut self) -> Option<usize> {
        let text = self
            .active_tab_grid()
            .map(|grid| grid.get_selected_text())
            .unwrap_or_default();
        self.copy_and_notify(&text)
    }

    /// Copia `text` (nada com texto vazio) e mostra o aviso.
    fn copy_and_notify(&mut self, text: &str) -> Option<usize> {
        if text.is_empty() {
            return None;
        }
        let copied = self
            .clipboard
            .as_ref()
            .map(|clipboard| clipboard.copy_selection_text(text));
        match copied {
            Some(Ok(())) => {
                let chars = text.chars().count();
                self.show_toast(
                    crate::ui::toast::copied_message(chars),
                    crate::ui::toast::ToastKind::Info,
                );
                Some(chars)
            }
            Some(Err(e)) => {
                tracing::warn!("Failed to copy: {}", e);
                self.show_toast(
                    crate::ui::toast::copy_failed(),
                    crate::ui::toast::ToastKind::Error,
                );
                None
            }
            None => {
                tracing::warn!("Failed to copy: clipboard not available");
                self.show_toast(
                    crate::ui::toast::copy_failed(),
                    crate::ui::toast::ToastKind::Error,
                );
                None
            }
        }
    }

    /// Copia texto diretamente para o clipboard
    pub fn copy_text(&self, text: impl Into<String>) -> Result<()> {
        let clipboard = self
            .clipboard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Clipboard not available"))?;

        let text = text.into();
        clipboard
            .copy_text(&text)
            .map_err(|e| anyhow::anyhow!("Failed to copy to clipboard: {}", e))?;

        trace!("Copied {} characters to clipboard", text.len());
        Ok(())
    }

    pub fn toggle_search(&mut self) {
        self.tab_manager.toggle_search();
    }

    pub fn start_search(&mut self) {
        self.tab_manager.start_search();
    }

    pub fn stop_search(&mut self) {
        self.tab_manager.stop_search();
    }

    pub fn update_search_query(&mut self, query: &str) {
        self.tab_manager.update_search_query(query);
    }

    pub fn next_search_match(&mut self) -> bool {
        self.tab_manager.next_search_match()
    }

    pub fn previous_search_match(&mut self) -> bool {
        self.tab_manager.previous_search_match()
    }

    pub fn get_current_match_position(&self) -> Option<(usize, usize, usize)> {
        self.tab_manager.get_current_match_position()
    }

    pub fn is_search_visible(&self) -> bool {
        self.tab_manager.is_search_visible()
    }

    pub fn is_search_active(&self) -> bool {
        self.tab_manager.is_search_active()
    }

    pub fn get_search_status(&self) -> String {
        self.tab_manager.get_search_status()
    }

    pub fn toggle_search_case_sensitive(&mut self) {
        self.tab_manager.toggle_case_sensitive();
    }

    pub fn toggle_search_whole_word(&mut self) {
        self.tab_manager.toggle_whole_word();
    }

    pub fn toggle_search_regex(&mut self) {
        self.tab_manager.toggle_regex();
    }

    pub fn get_search_query(&self) -> String {
        self.tab_manager.get_search_query()
    }

    // === Selection Methods ===

    /// Retorna referência mutável ao mouse selection handler
    pub fn mouse_selection_mut(&mut self) -> &mut MouseSelectionHandler {
        &mut self.mouse_selection
    }

    /// Manipula evento de seleção por mouse na tab ativa
    /// Este método resolve problemas de borrow entre mouse_selection e active_tab
    pub fn handle_mouse_selection_event(
        &mut self,
        event: crate::ui::mouse_selection::MouseSelectionEvent,
    ) {
        // Obtém referência mutável para o grid se houver tab ativa
        if let Some(grid) = self
            .tab_manager
            .get_active_tab_mut()
            .and_then(|t| t.grid_mut())
        {
            self.mouse_selection.handle_event(event, grid);
        }
    }

    /// Verifica se há seleção ativa na tab ativa
    pub fn has_selection(&self) -> bool {
        self.active_tab_grid()
            .map(|grid| grid.has_selection())
            .unwrap_or(false)
    }

    /// Copia a seleção atual para o clipboard
    pub fn copy_selection(&self) -> Result<()> {
        let clipboard = self
            .clipboard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Clipboard not available"))?;

        let content = if let Some(grid) = self.active_tab_grid() {
            grid.get_selected_text()
        } else {
            String::new()
        };

        if content.is_empty() {
            trace!("No selection to copy");
            return Ok(());
        }

        clipboard
            .copy_text(&content)
            .map_err(|e| anyhow::anyhow!("Failed to copy selection: {}", e))?;

        trace!("Copied {} characters to clipboard", content.len());
        Ok(())
    }

    /// Seleciona todo o texto na tab ativa
    pub fn select_all(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            if let Some(grid) = tab.grid_mut() {
                grid.select_all();
            }
        }
    }

    /// Cancela a seleção atual
    pub fn clear_selection(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            if let Some(grid) = tab.grid_mut() {
                grid.clear_selection();
            }
        }
        self.mouse_selection.reset();
    }

    // === Quick Connect Methods ===

    /// Verifica se o Quick Connect está visível
    pub fn is_quick_connect_visible(&self) -> bool {
        self.quick_connect_ui
            .as_ref()
            .is_some_and(|ui| ui.is_visible())
    }

    /// Abre o diálogo de Quick Connect
    pub fn show_quick_connect(&mut self) {
        if self.quick_connect_ui.is_none() {
            self.quick_connect_ui = Some(QuickConnectUI::new(Arc::clone(&self.ssh_manager)));
        }

        self.quick_connect_navigated = false;
        if let Some(ref mut ui) = self.quick_connect_ui {
            ui.show();
            info!("Quick Connect dialog opened");
        }
    }

    /// Fecha o diálogo de Quick Connect
    pub fn hide_quick_connect(&mut self) {
        if let Some(ref mut ui) = self.quick_connect_ui {
            ui.hide();
        }
    }

    /// Alterna a visibilidade do Quick Connect
    pub fn toggle_quick_connect(&mut self) {
        if self.is_quick_connect_visible() {
            self.hide_quick_connect();
        } else {
            self.show_quick_connect();
        }
    }

    /// Processa uma tecla para o Quick Connect
    /// Retorna true se a tecla foi consumida pelo Quick Connect
    pub fn handle_quick_connect_key(&mut self, key: QuickConnectKey) -> bool {
        if matches!(key, QuickConnectKey::Up | QuickConnectKey::Down) {
            self.quick_connect_navigated = true;
        }
        let navigated = self.quick_connect_navigated;
        if key == QuickConnectKey::Enter {
            let params = match self.quick_connect_ui.as_mut() {
                Some(ui) if ui.is_visible() => {
                    match prepare_quick_connect(ui, navigated).validate() {
                        Ok(params) => params,
                        Err(e) => {
                            warn!("Quick Connect validation failed: {}", e);
                            ui.set_error_message(e.to_string());
                            return true;
                        }
                    }
                }
                _ => return false,
            };

            match self.create_ssh_tab(&params) {
                Ok(()) => {
                    if let Some(ui) = self.quick_connect_ui.as_mut() {
                        ui.complete_connection(&params);
                    }
                }
                Err(e) => {
                    warn!(error = ?e, "Failed to start SSH tab from Quick Connect");
                    if let Some(ui) = self.quick_connect_ui.as_mut() {
                        ui.set_error_message(format!("Failed to start SSH: {}", e));
                    }
                }
            }

            return true;
        }

        if let Some(ref mut ui) = self.quick_connect_ui {
            if ui.is_visible() {
                return ui.handle_key(key);
            }
        }
        false
    }

    /// Retorna referência à UI do Quick Connect
    pub fn quick_connect_ui(&self) -> Option<&QuickConnectUI> {
        self.quick_connect_ui.as_ref()
    }

    /// Retorna referência mutável à UI do Quick Connect
    pub fn quick_connect_ui_mut(&mut self) -> Option<&mut QuickConnectUI> {
        self.quick_connect_ui.as_mut()
    }

    /// Preenche o Quick Connect com uma string de conexão
    pub fn quick_connect_fill(&mut self, input: &str) {
        if let Some(ref mut ui) = self.quick_connect_ui {
            ui.parse_and_fill(input);
        }
    }

    /// Retorna o gerenciador SSH
    pub fn ssh_manager(&self) -> Arc<RwLock<SshManager>> {
        Arc::clone(&self.ssh_manager)
    }

    // === Command Palette Methods ===

    /// Verifica se a command palette está visível
    pub fn is_command_palette_visible(&self) -> bool {
        self.command_palette.is_visible()
    }

    /// Abre a command palette
    pub fn show_command_palette(&mut self) {
        self.command_palette.open();
    }

    /// Fecha a command palette (na escolha de tema, volta ao tema anterior).
    pub fn hide_command_palette(&mut self) {
        self.command_palette.close();
        self.finish_theme_preview(false);
        self.custom_color = None;
    }

    /// Alterna a visibilidade da command palette
    pub fn toggle_command_palette(&mut self) {
        if self.command_palette.is_visible() {
            self.hide_command_palette();
        } else {
            self.command_palette.open();
        }
    }

    /// "Choose Theme…": a paleta passa a listar os temas, com o atual
    /// selecionado. Passar por um tema mostra a prévia; Enter grava, Esc volta.
    pub fn open_theme_chooser(&mut self) {
        if self.theme_before_preview.is_none() {
            self.theme_before_preview = Some(self.config.colors.clone());
        }
        let current = self.config.colors.name.clone();
        self.command_palette.open_themes(&current);
    }

    /// Se a paleta está mostrando a lista de temas.
    pub fn is_choosing_theme(&self) -> bool {
        self.command_palette.is_visible()
            && self.command_palette.mode() == crate::ui::command_palette::PaletteMode::Themes
    }

    /// Aplica (sem gravar) o tema selecionado na lista de temas.
    fn preview_selected_theme(&mut self) {
        if !self.is_choosing_theme() {
            return;
        }
        let Some(theme) = self.command_palette.selected_theme() else {
            return;
        };
        if self.config.colors.name.eq_ignore_ascii_case(theme) {
            return;
        }
        if crate::config::theme::Theme::apply_to_config(theme, &mut self.config) {
            self.config_dirty = true;
        }
    }

    /// Fim da escolha de tema: `commit` grava o tema da prévia; senão as
    /// cores de antes voltam. Sem escolha em andamento, não faz nada.
    fn finish_theme_preview(&mut self, commit: bool) {
        let Some(before) = self.theme_before_preview.take() else {
            return;
        };
        if commit {
            if self.config.colors != before {
                if let Err(e) = self.config.save() {
                    tracing::warn!(error = %e, "Failed to save theme");
                }
            }
        } else if self.config.colors != before {
            self.config.colors = before;
            self.config_dirty = true;
        }
    }

    /// Lembra a rolagem das listas desenhadas no último frame: o próximo
    /// desenho parte dela (o mouse não faz a lista voltar ao topo).
    pub fn remember_overlay_scroll(&mut self, hits: &crate::ui::overlay_hits::OverlayHits) {
        for hit in hits.iter() {
            let Some(list) = hit.list else {
                continue;
            };
            match hit.kind {
                OverlayKind::CommandPalette => self.command_palette.set_first_visible(list.first),
                OverlayKind::QuickConnect => {
                    if let Some(ui) = self.quick_connect_ui.as_mut() {
                        ui.set_first_visible(list.first);
                    }
                }
                OverlayKind::Settings => {
                    if let Some(screen) = self.settings_screen.as_mut() {
                        screen.set_first_visible(list.first);
                    }
                }
                _ => {}
            }
        }
    }

    /// Retorna referência à command palette
    pub fn command_palette(&self) -> &CommandPalette {
        &self.command_palette
    }

    /// Retorna referência mutável à command palette
    pub fn command_palette_mut(&mut self) -> &mut CommandPalette {
        &mut self.command_palette
    }

    /// Processa uma tecla para a command palette.
    /// Retorna a ação a ser executada, ou None se a tecla foi apenas consumida.
    pub fn handle_command_palette_key(
        &mut self,
        key: &crate::ui::command_palette::PaletteInputKey,
    ) -> PaletteAction {
        use crate::ui::command_palette::PaletteInputKey;
        match key {
            PaletteInputKey::Char(c) => {
                self.command_palette.type_char(*c);
                PaletteAction::None
            }
            PaletteInputKey::Backspace => {
                self.command_palette.backspace();
                PaletteAction::None
            }
            PaletteInputKey::Up => {
                self.command_palette.move_up();
                self.preview_selected_theme();
                PaletteAction::None
            }
            PaletteInputKey::Down => {
                self.command_palette.move_down();
                self.preview_selected_theme();
                PaletteAction::None
            }
            PaletteInputKey::Enter => {
                use crate::ui::command_palette::PaletteCommand;
                self.preview_selected_theme();
                let hex = self.command_palette.hex_color();
                let command = self.command_palette.confirm();
                match command {
                    Some(PaletteCommand::ApplyHexColor) => {
                        self.apply_custom_tab_color(hex);
                        PaletteAction::None
                    }
                    Some(PaletteCommand::ChooseTheme) => {
                        self.open_theme_chooser();
                        PaletteAction::None
                    }
                    Some(PaletteCommand::SetTheme(_)) => {
                        self.finish_theme_preview(true);
                        PaletteAction::None
                    }
                    // Enter sem tema na lista (filtro vazio): desiste.
                    None if self.theme_before_preview.is_some() => {
                        self.hide_command_palette();
                        PaletteAction::None
                    }
                    other => PaletteAction::from_command(other),
                }
            }
            PaletteInputKey::Escape => {
                self.hide_command_palette();
                PaletteAction::None
            }
        }
    }

    // === Onboarding Methods ===

    /// Verifica se o onboarding está visível
    pub fn is_onboarding_visible(&self) -> bool {
        self.onboarding.is_visible()
    }

    /// Passa para o próximo tema embutido, aplica na hora e salva.
    pub fn toggle_theme(&mut self) -> &str {
        let next = crate::ui::settings::next_theme(&self.config.colors.name);
        if crate::config::theme::Theme::apply_to_config(next, &mut self.config) {
            self.config_dirty = true;
            if let Err(e) = self.config.save() {
                tracing::warn!(error = %e, "Failed to save theme");
            }
        }
        &self.config.colors.name
    }

    /// Informa a todas as tabs as cores do tema atual (OSC 10/11/12).
    pub fn refresh_theme_colors(&mut self) {
        let colors = self.config.colors.clone();
        for index in 0..self.tab_manager.tab_count() {
            if let Some(grid) = self
                .tab_manager
                .get_tab_mut(index)
                .and_then(|tab| tab.grid_mut())
            {
                crate::terminal::buffer::apply_theme_colors(grid, &colors);
            }
        }
    }

    /// Mostra ou esconde o onboarding sem gravar a escolha (snapshots).
    pub fn set_onboarding_visible(&mut self, visible: bool) {
        self.onboarding.set_visible(visible);
    }

    /// Fecha o onboarding
    pub fn dismiss_onboarding(&mut self) {
        self.onboarding.dismiss();
    }

    /// Retorna referência ao onboarding
    pub fn onboarding(&self) -> &Onboarding {
        &self.onboarding
    }

    // === Mouse sobre painéis abertos ===

    /// Se o painel modal `kind` está aberto agora. O mapa de cliques é do
    /// último frame: o painel pode ter fechado (Esc) antes do próximo desenho.
    pub fn is_overlay_visible(&self, kind: OverlayKind) -> bool {
        match kind {
            OverlayKind::CommandPalette => self.is_command_palette_visible(),
            OverlayKind::QuickConnect => self.is_quick_connect_visible(),
            OverlayKind::Settings => self.is_settings_visible(),
            OverlayKind::Notifications => self.is_notifications_panel_visible(),
            OverlayKind::Assistant => self.assistant_view().is_some(),
            OverlayKind::CloseConfirmation => self.has_pending_close_tab(),
            OverlayKind::Onboarding => self.is_onboarding_visible(),
            OverlayKind::TabMenu => self.is_tab_menu_visible(),
        }
    }

    /// Fecha o painel `kind`, como o Esc (clique fora do painel).
    pub fn close_overlay(&mut self, kind: OverlayKind) {
        match kind {
            OverlayKind::CommandPalette => self.hide_command_palette(),
            OverlayKind::QuickConnect => self.hide_quick_connect(),
            OverlayKind::Settings => self.hide_settings(),
            OverlayKind::Notifications => self.notifications_panel_visible = false,
            OverlayKind::Assistant => {
                self.assistant_key(AssistantKey::Escape);
            }
            OverlayKind::CloseConfirmation => self.cancel_close_pending_tab(),
            OverlayKind::Onboarding => self.dismiss_onboarding(),
            OverlayKind::TabMenu => self.close_tab_menu(),
        }
    }

    /// Destaca o item `index` da lista do painel (mouse sobre o item).
    pub fn hover_overlay_item(&mut self, kind: OverlayKind, index: usize) {
        match kind {
            OverlayKind::CommandPalette => {
                self.command_palette.select(index);
                self.preview_selected_theme();
            }
            OverlayKind::TabMenu => self.select_tab_menu_item(index),
            OverlayKind::QuickConnect => {
                if let Some(ui) = self.quick_connect_ui.as_mut() {
                    ui.select_index(index);
                }
            }
            OverlayKind::Settings => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.select(index);
                }
            }
            _ => {}
        }
    }

    /// Rola a lista do painel `lines` itens (positivo = para baixo), parando
    /// nas pontas. Painel sem lista rolável ignora.
    pub fn scroll_overlay(&mut self, kind: OverlayKind, lines: i32) {
        match kind {
            OverlayKind::CommandPalette => {
                self.command_palette.scroll_by(lines);
                self.preview_selected_theme();
            }
            OverlayKind::TabMenu => self.move_tab_menu_selection(lines),
            OverlayKind::QuickConnect => {
                if let Some(ui) = self.quick_connect_ui.as_mut() {
                    for _ in 0..lines.unsigned_abs() {
                        if lines > 0 {
                            ui.select_down();
                        } else {
                            ui.select_up();
                        }
                    }
                }
            }
            OverlayKind::Settings => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.scroll_by(lines);
                }
            }
            _ => {}
        }
    }

    /// Clique num host do seletor SSH: seleciona e conecta, como o Enter
    /// depois de escolher com as setas.
    pub fn activate_quick_connect_item(&mut self, index: usize) -> bool {
        let Some(ui) = self.quick_connect_ui.as_mut() else {
            return false;
        };
        ui.select_index(index);
        self.quick_connect_navigated = true;
        self.handle_quick_connect_key(QuickConnectKey::Enter)
    }
}

/// Converte um grid para string (conteúdo visível)
/// Antes de conectar: Enter pega o item destacado da lista (como num seletor);
/// sem itens, `usuário@host:porta` digitado é quebrado nos campos.
/// Só usa a lista se o usuário filtrou (digitou) ou navegou com as setas:
/// com o campo vazio, Enter não conecta sozinho no primeiro host salvo.
fn prepare_quick_connect(ui: &mut QuickConnectUI, navigated: bool) -> &mut QuickConnectUI {
    use crate::ssh::QuickConnectField;
    let typed = ui.get_field(QuickConnectField::Host).to_string();
    if ui.has_results() && (navigated || !typed.is_empty()) {
        ui.apply_selection();
    } else if typed.contains('@') || typed.contains(':') {
        ui.parse_and_fill(&typed);
    }
    ui
}

/// O que o painel do assistente de IA mostra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssistantView {
    /// Campo para descrever o comando desejado.
    Prompt(String),
    /// Esperando a resposta (título da tarefa).
    Waiting(&'static str),
    /// Resposta; `command` é o comando sugerido (Enter insere no terminal).
    Answer {
        title: &'static str,
        text: String,
        command: Option<String>,
    },
    Error(String),
}

/// Teclas do painel do assistente.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssistantKey {
    Char(char),
    Backspace,
    Enter,
    Escape,
}

/// Diretório de um painel: OSC 7 do shell, senão /proc.
fn buffer_cwd(buffer: &TerminalBuffer) -> Option<std::path::PathBuf> {
    buffer
        .grid()
        .working_dir()
        .map(std::path::Path::to_path_buf)
        .or_else(|| buffer.pty().current_dir())
}

/// Aviso lido de um painel, ainda não registrado.
struct PendingNotification {
    tab_id: u32,
    tab_title: String,
    pane: PaneId,
    /// `None` é a campainha (BEL).
    note: Option<crate::terminal::grid::TerminalNotification>,
    seen: bool,
}

/// Esvazia os avisos e a campainha de todos os painéis de uma tab.
/// `visible` indica que a tab é a ativa numa janela em foco.
fn drain_tab_notifications(
    tab: &mut crate::ui::tabs::Tab,
    visible: bool,
) -> Vec<PendingNotification> {
    let (tab_id, focused_pane, tab_title) = (tab.id, tab.focused_pane(), tab.title());
    let mut out = Vec::new();
    for pane in tab.pane_ids() {
        let Some(grid) = tab.pane_buffer_mut(pane).map(|b| b.grid_mut()) else {
            continue;
        };
        let seen = visible && pane == focused_pane;
        let bell = grid.take_bell().then_some(None);
        let notes = grid.take_notifications().into_iter().map(Some);
        for note in notes.chain(bell) {
            out.push(PendingNotification {
                tab_id,
                tab_title: tab_title.clone(),
                pane,
                note,
                seen,
            });
        }
    }
    out
}

/// Recria uma tab salva: cada painel abre um shell no diretório salvo (se
/// ainda existir) ou anexa de novo à sua sessão do multiplexer. Devolve a
/// tab e os painéis anexados a sessões.
fn restore_tab(
    config: &Config,
    snapshot: &crate::session::workspace::TabSnapshot,
    cols: u16,
    rows: u16,
) -> Result<(crate::ui::tabs::Tab, Vec<(PaneId, String)>)> {
    let mut panes = Vec::new();
    let mut focused = None;
    let mut sessions = Vec::new();
    let mut failure = None;
    let layout = PaneLayout::import(&snapshot.layout, |pane| {
        match spawn_restored_pane(config, cols, rows, pane) {
            Ok((id, buffer)) => {
                if pane.focused {
                    focused = Some(id);
                }
                if let Some(name) = &pane.mux_session {
                    sessions.push((id, name.clone()));
                }
                panes.push((id, buffer));
                id
            }
            Err(e) => {
                failure = Some(e);
                0
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    let first = panes.first().map(|(id, _)| *id).unwrap_or(0);
    let tab = crate::ui::tabs::Tab::from_panes("bash", layout, focused.unwrap_or(first), panes);
    Ok((tab, sessions))
}

fn spawn_restored_pane(
    config: &Config,
    cols: u16,
    rows: u16,
    pane: &crate::session::workspace::PaneSnapshot,
) -> Result<(PaneId, TerminalBuffer)> {
    let cwd = pane.cwd.as_deref().filter(|dir| dir.is_dir());
    let Some(name) = pane.mux_session.as_deref() else {
        return spawn_pane(config, cols, rows, None, cwd);
    };
    // Tab persistente: anexa à sessão viva; sem ela (reiniciou a máquina),
    // um shell novo no daemon com o mesmo nome, no diretório de antes.
    #[cfg(unix)]
    {
        if let Ok(attached) = attach_persistent_pane(config, cols, rows, name) {
            return Ok(attached);
        }
        if let Ok(spawned) = spawn_persistent_pane(config, cols, rows, cwd, name, None) {
            return Ok(spawned);
        }
    }
    // Sessão antiga do `mux attach` (grid no daemon): o CLI anexa no PTY.
    let (program, args) = mux_attach_command(name);
    spawn_pane(config, cols, rows, Some((&program, &args)), cwd)
}

/// Painel anexado a uma sessão persistente que já roda no daemon.
#[cfg(unix)]
fn attach_persistent_pane(
    config: &Config,
    cols: u16,
    rows: u16,
    session: &str,
) -> Result<(PaneId, TerminalBuffer)> {
    let pty = crate::terminal::pty::Pty::connect_persistent(session, cols, rows, None)?;
    let pane = crate::ui::tabs::allocate_pane_id();
    Ok((pane, TerminalBuffer::with_pty(pty, cols, rows, config)))
}

/// Painel novo de tab persistente: o shell nasce no daemon, com o ambiente
/// deste processo (o daemon roda fora da sessão gráfica).
#[cfg(unix)]
fn spawn_persistent_pane(
    config: &Config,
    cols: u16,
    rows: u16,
    cwd: Option<&std::path::Path>,
    session: &str,
    command: Option<Vec<String>>,
) -> Result<(PaneId, TerminalBuffer)> {
    let pane = crate::ui::tabs::allocate_pane_id();
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(key, _)| key != crate::multiplexer::daemon::SESSION_ENV)
        .collect();
    env.extend(pane_env(pane));
    let cwd = cwd
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let spec = crate::multiplexer::protocol::StreamCreate {
        cwd: cwd.map(|dir| dir.display().to_string()),
        command,
        env,
        ..Default::default()
    };
    let pty = crate::terminal::pty::Pty::connect_persistent(session, cols, rows, Some(spec))?;
    Ok((pane, TerminalBuffer::with_pty(pty, cols, rows, config)))
}

#[cfg(not(unix))]
fn spawn_persistent_pane(
    _config: &Config,
    _cols: u16,
    _rows: u16,
    _cwd: Option<&std::path::Path>,
    _session: &str,
    _command: Option<Vec<String>>,
) -> Result<(PaneId, TerminalBuffer)> {
    anyhow::bail!("persistent tabs are not available on this system yet")
}

/// URL http/https que cobre a coluna `col` (colunas em caracteres).
fn link_in_text(text: &str, col: usize) -> Option<String> {
    let detector = crate::ui::url_detection::UrlDetector::new();
    detector
        .detect_in_text(text, 0, 0)
        .into_iter()
        .filter(|found| found.protocol.starts_with("http"))
        .find(|found| {
            // O detector conta bytes; a coluna conta caracteres.
            let start = text
                .get(..found.start_col)
                .map_or(0, |prefix| prefix.chars().count());
            let end = start + found.url.chars().count();
            (start..end).contains(&col)
        })
        .map(|found| found.url)
}

fn open_link_blocking(url: &str, target: crate::config::LinkTarget) {
    let result = match target {
        crate::config::LinkTarget::Builtin => {
            crate::browser::remote::open_in_browser(url).map(|_| ())
        }
        crate::config::LinkTarget::System => {
            crate::ui::url_detection::open_url(url).map_err(|e| e.to_string())
        }
    };
    match result {
        Ok(()) => tracing::info!(url, "Link opened"),
        Err(e) => tracing::warn!(url, error = %e, "Failed to open link"),
    }
}

/// Comando que anexa (criando se preciso) à sessão `name` do multiplexer: o
/// `code-terminal-cli` ao lado do executável atual, ou o do PATH.
pub fn mux_attach_command(name: &str) -> (String, Vec<String>) {
    let cli_name = if cfg!(windows) {
        "code-terminal-cli.exe"
    } else {
        "code-terminal-cli"
    };
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(cli_name)))
        .filter(|path| path.is_file())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| cli_name.to_string());
    let args = vec![
        "mux-attach".to_string(),
        name.to_string(),
        "--create".to_string(),
    ];
    (program, args)
}

/// Texto de uma linha do grid (sem as células de continuação de caracteres
/// largos e sem espaços à direita).
fn row_text(row: &[crate::terminal::grid::Cell]) -> String {
    row.iter()
        .filter(|cell| !cell.spacer)
        .map(|cell| if cell.char == '\0' { ' ' } else { cell.char })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// A área de transferência não tem texto (vazia ou só uma imagem).
#[derive(Debug, thiserror::Error)]
#[error("clipboard has no text to paste")]
pub struct NothingToPaste;

/// Uma seleção vale ser copiada ao soltar o mouse? Não quando é só o
/// clique (seleção normal com início igual ao fim), que deixa o caractere
/// sob o cursor marcado; palavra (duplo clique) e linha (triplo) valem.
pub fn selection_worth_copying(selection: &crate::terminal::grid::Selection) -> bool {
    use crate::terminal::grid::SelectionKind;
    selection.active
        && !(selection.kind == SelectionKind::Normal && selection.start == selection.end)
}

/// Acorda o app periodicamente para o `update` dos plugins (uma thread só,
/// iniciada no primeiro plugin carregado).
fn start_plugin_clock() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("plugin-clock".into())
            .spawn(|| loop {
                std::thread::sleep(crate::app::plugins::UPDATE_INTERVAL);
                crate::terminal::pty::request_redraw();
            });
        if let Err(error) = spawned {
            tracing::warn!(error = %error, "Failed to start plugin clock");
        }
    });
}

/// Variáveis que dizem ao processo em que painel ele roda e onde está a
/// Socket API (o `code-terminal-cli notify` usa para acender o painel certo).
pub fn pane_env(pane: PaneId) -> Vec<(String, String)> {
    let mut env = vec![("CODE_TERMINAL_PANE_ID".to_string(), pane.to_string())];
    env.extend(socket_env());
    env
}

#[cfg(unix)]
fn socket_env() -> Option<(String, String)> {
    Some((
        "CODE_TERMINAL_SOCKET".to_string(),
        crate::multiplexer::socket_api::server::default_unix_socket_path()
            .display()
            .to_string(),
    ))
}

/// No Windows a Socket API usa TCP local; o CLI acha o endereço sozinho.
#[cfg(not(unix))]
fn socket_env() -> Option<(String, String)> {
    None
}

/// Novo painel: id único e PTY já com as variáveis do painel.
fn spawn_pane(
    config: &Config,
    cols: u16,
    rows: u16,
    command: Option<(&str, &[String])>,
    cwd: Option<&std::path::Path>,
) -> Result<(PaneId, TerminalBuffer)> {
    let pane = crate::ui::tabs::allocate_pane_id();
    let buffer = TerminalBuffer::for_pane(cols, rows, config, command, cwd, &pane_env(pane))?;
    Ok((pane, buffer))
}

/// Coleta os metadados de uma tab para a sidebar.
fn tab_sidebar_info(
    tab: &crate::ui::tabs::Tab,
    home: Option<&std::path::Path>,
    unread: usize,
    git_status: &crate::ui::sidebar_info::GitStatusCache,
) -> TabSidebarInfo {
    let cwd = tab.buffer.as_ref().and_then(|buffer| {
        buffer
            .grid()
            .working_dir()
            .map(std::path::Path::to_path_buf)
            .or_else(|| buffer.pty().current_dir())
    });
    TabSidebarInfo {
        title: tab.title(),
        branch: cwd.as_deref().and_then(sidebar_info::git_branch),
        git: cwd
            .as_deref()
            .filter(|dir| sidebar_info::git_branch(dir).is_some())
            .and_then(|dir| git_status.get(dir)),
        cwd: cwd
            .as_deref()
            .map(|dir| sidebar_info::shorten_home(dir, home)),
        ports: sidebar_info::listening_ports_of(&tab.pane_pids()),
        unread,
        panes: tab.pane_count(),
        memory_kb: None,
        persistent: false,
    }
}

fn grid_to_string(grid: &crate::terminal::grid::Grid) -> String {
    let visible = grid.visible_lines();
    let mut result = String::new();

    for row in visible.iter() {
        let mut line = String::new();
        for cell in row.iter().filter(|cell| !cell.spacer) {
            if cell.char == '\0' || cell.char == ' ' {
                line.push(' ');
            } else {
                line.push(cell.char);
            }
        }
        // Trim trailing spaces
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            result.push_str(trimmed);
        }
        result.push('\n');
    }

    // Remove última newline se existir
    if result.ends_with('\n') {
        result.pop();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regressão: `exit` no shell deixava a tab morta aberta para sempre.
    #[cfg(unix)]
    fn sleeper(app: &AppState) -> TerminalBuffer {
        TerminalBuffer::with_command(
            80,
            24,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
            &app.config,
        )
        .unwrap()
    }

    /// Dividir cria um painel novo em foco e redimensiona os dois PTYs.
    #[cfg(unix)]
    #[test]
    fn test_split_resize_focus_and_close_panes() {
        let mut app = AppState::with_command(
            Config::default(),
            81,
            24,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        let first = app.active_tab().unwrap().focused_pane();
        app.split_active_pane_running(
            SplitDirection::Horizontal,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.pane_count(), 2);
        let second = tab.focused_pane();
        assert_ne!(first, second);
        // A primeira linha de cada painel é a barra de título (#32).
        assert_eq!(tab.pane_buffer(first).unwrap().content_size(), (40, 23));
        assert_eq!(tab.pane_buffer(second).unwrap().content_size(), (40, 23));

        // Foco por direção e por clique.
        assert!(app.focus_pane_direction(FocusDirection::Left));
        assert_eq!(app.active_tab().unwrap().focused_pane(), first);
        assert_eq!(app.pane_at(5, 50), Some((second, 4, 9)));
        assert_eq!(app.pane_at(5, 40), None);
        // Clique na barra de título não vai para o terminal.
        assert_eq!(app.pane_at(0, 50), None);
        assert!(app.focus_pane(second));
        assert_eq!(app.focused_pane_cell(5, 10), (4, 0));
        assert!(app.cycle_pane(true));
        assert_eq!(app.active_tab().unwrap().focused_pane(), first);

        // Redimensionar o painel em foco e a janela.
        assert!(app.resize_active_pane(true));
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.pane_buffer(first).unwrap().content_size().0, 44);
        app.resize(121, 30).unwrap();
        let tab = app.active_tab().unwrap();
        let (a, b) = (
            tab.pane_buffer(first).unwrap().content_size(),
            tab.pane_buffer(second).unwrap().content_size(),
        );
        assert_eq!(a.0 + b.0 + 1, 121);
        // A barra de título ocupa a primeira linha de cada painel (#32).
        assert_eq!((a.1, b.1), (29, 29));

        // Fechar um painel devolve a área ao vizinho; o último fecha a tab.
        assert!(!app.close_active_pane());
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.pane_count(), 1);
        assert_eq!(tab.focused_pane(), second);
        assert_eq!(tab.pane_buffer(second).unwrap().content_size(), (121, 30));
        assert!(app.close_active_pane());
        assert_eq!(app.tab_count(), 0);
    }

    /// Painel cujo processo saiu fecha sozinho; os outros continuam.
    #[cfg(unix)]
    #[test]
    fn test_exited_pane_closes_and_neighbor_takes_space() {
        let mut app = AppState::with_command(
            Config::default(),
            81,
            24,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        app.split_active_pane_running(
            SplitDirection::Vertical,
            "/bin/sh",
            &["-c".into(), "exit 0".into()],
        )
        .unwrap();
        let other = sleeper(&app);
        drop(other);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.active_tab().unwrap().pane_count() == 2 && std::time::Instant::now() < deadline {
            assert!(!app.close_exited_tabs());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.pane_count(), 1);
        let id = tab.focused_pane();
        assert_eq!(tab.pane_buffer(id).unwrap().content_size(), (81, 24));
    }

    /// Aviso de um painel fora de foco acende o anel e o contador; ir até ele
    /// apaga.
    #[cfg(unix)]
    #[test]
    fn test_notifications_ring_unread_and_jump() {
        let mut app = AppState::with_command(
            Config::default(),
            81,
            24,
            "/bin/sh",
            &[
                "-c".into(),
                "sleep 1; printf '\\033]9;Claude needs input\\007'; sleep 30".into(),
            ],
        )
        .unwrap();
        let noisy = app.active_tab().unwrap().focused_pane();
        app.split_active_pane_running(
            SplitDirection::Horizontal,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.notifications().has_unread(noisy) && std::time::Instant::now() < deadline {
            app.process_output();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(app.notifications().has_unread(noisy));
        let tab_id = app.active_tab().unwrap().id;
        assert_eq!(app.notifications().unread_for_tab(tab_id), 1);
        app.refresh_sidebar(true);
        assert_eq!(app.sidebar_info()[0].unread, 1);

        assert!(app.jump_to_unread());
        assert_eq!(app.active_tab().unwrap().focused_pane(), noisy);
        assert!(!app.notifications().has_unread(noisy));
        assert!(!app.jump_to_unread());
        let last = app.notifications().history().next().unwrap();
        assert_eq!(last.summary(), "Claude needs input");
    }

    /// O processo do painel recebe o id dele (para o CLI acender o painel certo).
    #[cfg(unix)]
    #[test]
    fn test_pane_processes_know_their_pane_id() {
        let mut app = AppState::with_command(
            Config::default(),
            80,
            5,
            "/bin/sh",
            &[
                "-c".into(),
                "echo pane=$CODE_TERMINAL_PANE_ID; sleep 30".into(),
            ],
        )
        .unwrap();
        let pane = app.active_tab().unwrap().focused_pane();
        let expected = format!("pane={pane}");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            app.process_output();
            text = grid_to_string(app.active_tab_grid().unwrap());
            if text.contains(&expected) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(text.contains(&expected), "{text}");
        assert!(pane_env(pane)
            .iter()
            .any(|(k, _)| k == "CODE_TERMINAL_PANE_ID"));
    }

    /// Fechar e reabrir volta às mesmas tabs, painéis, proporções e pastas.
    #[cfg(unix)]
    #[test]
    fn test_link_in_text_uses_character_columns() {
        let text = "ação → https://exemplo.com/a?b=1 fim";
        let start = text.chars().position(|c| c == 'h').unwrap();
        assert_eq!(
            link_in_text(text, start).as_deref(),
            Some("https://exemplo.com/a?b=1")
        );
        assert!(link_in_text(text, start + 24).is_some());
        assert_eq!(link_in_text(text, start + 26), None, "depois do link");
        assert_eq!(link_in_text(text, 0), None);
        assert_eq!(link_in_text("ftp://x.y/z", 2), None, "só http/https");
    }

    #[cfg(unix)]
    #[test]
    fn test_link_at_reads_the_focused_grid() {
        let mut app = AppState::with_command(Config::default(), 60, 4, "/bin/cat", &[]).unwrap();
        app.active_tab_mut()
            .and_then(|tab| tab.buffer.as_mut())
            .unwrap()
            .inject_output("veja 日本 https://a.io/x\r\n".as_bytes());
        // "veja " (5) + 日本 (4 células) + espaço = coluna 10.
        assert_eq!(app.link_at(0, 10).as_deref(), Some("https://a.io/x"));
        assert_eq!(app.link_at(0, 3), None);
        assert_eq!(app.link_at(9, 0), None);
    }

    /// "Choose Theme…": passar pelos temas mostra a prévia, Esc volta às
    /// cores de antes e Enter fica com o tema (#22).
    #[test]
    fn test_choose_theme_previews_and_restores() {
        use crate::ui::command_palette::{PaletteInputKey, PaletteMode, THEMES};
        let mut app = AppState::new(40, 5).unwrap();
        app.config.colors.name = "dark".into();
        let before = app.config.colors.clone();
        app.show_command_palette();
        for c in "tema".chars() {
            app.handle_command_palette_key(&PaletteInputKey::Char(c));
        }
        assert_eq!(
            app.command_palette().selected_item().unwrap().command,
            crate::ui::command_palette::PaletteCommand::ChooseTheme,
            "\"tema\" acha o Choose Theme… primeiro"
        );
        assert_eq!(
            app.handle_command_palette_key(&PaletteInputKey::Enter),
            PaletteAction::None
        );
        assert!(app.is_choosing_theme());
        assert_eq!(app.command_palette().items().len(), THEMES.len());
        assert_eq!(app.command_palette().selected_theme(), Some("dark"));

        app.config_dirty = false;
        app.handle_command_palette_key(&PaletteInputKey::Down);
        assert_eq!(app.config.colors.name, "light", "prévia na hora");
        assert!(app.config_dirty);
        // Mouse sobre outro tema e roda também mostram a prévia.
        app.hover_overlay_item(OverlayKind::CommandPalette, 2);
        assert_eq!(app.config.colors.name, THEMES[2]);
        app.scroll_overlay(OverlayKind::CommandPalette, 1);
        assert_eq!(app.config.colors.name, THEMES[3]);

        app.handle_command_palette_key(&PaletteInputKey::Escape);
        assert!(!app.is_command_palette_visible());
        assert_eq!(app.config.colors, before, "Esc volta ao tema anterior");

        // Enter fica com o tema; clicar fora (fechar) depois não desfaz.
        app.open_theme_chooser();
        app.handle_command_palette_key(&PaletteInputKey::Down);
        app.handle_command_palette_key(&PaletteInputKey::Enter);
        assert!(!app.is_command_palette_visible());
        assert_eq!(app.config.colors.name, "light");
        app.close_overlay(OverlayKind::CommandPalette);
        assert_eq!(app.config.colors.name, "light");

        // Clicar fora no meio da escolha também volta.
        app.open_theme_chooser();
        app.hover_overlay_item(OverlayKind::CommandPalette, 5);
        app.close_overlay(OverlayKind::CommandPalette);
        assert_eq!(app.config.colors.name, "light");
        assert_eq!(app.command_palette().mode(), PaletteMode::Themes);
        app.show_command_palette();
        assert_eq!(app.command_palette().mode(), PaletteMode::Commands);
    }

    /// A rolagem desenhada fica guardada: o mouse sobre a primeira linha
    /// visível depois de rolar não volta a lista para o topo (#22).
    #[test]
    fn test_overlay_scroll_is_remembered_between_frames() {
        use crate::ui::overlay_hits::{
            follow_selection, route_motion, ListGeometry, OverlayHits, OverlayMouse, Rect,
        };
        let mut app = AppState::new(40, 5).unwrap();
        app.show_command_palette();
        let total = app.command_palette().items().len();
        let visible = 10;
        let frame = |app: &AppState| {
            let palette = app.command_palette();
            let first = follow_selection(
                palette.first_visible(),
                palette.selected_index,
                total,
                visible,
            );
            let mut hits = OverlayHits::new();
            hits.push(
                OverlayKind::CommandPalette,
                Rect::new(0.0, 0.0, 400.0, 400.0),
                Some(ListGeometry {
                    x: 0.0,
                    y: 50.0,
                    width: 400.0,
                    row_height: 30.0,
                    first,
                    visible,
                    total,
                }),
            );
            hits
        };
        app.scroll_overlay(OverlayKind::CommandPalette, 15);
        let hits = frame(&app);
        app.remember_overlay_scroll(&hits);
        let first = app.command_palette().first_visible();
        assert_eq!(first, 15 + 1 - visible);
        let OverlayMouse::Hover(kind, index) = route_motion(&hits, 10.0, 55.0) else {
            panic!("esperava Hover");
        };
        app.hover_overlay_item(kind, index);
        assert_eq!(app.command_palette().selected_index, first);
        let hits = frame(&app);
        app.remember_overlay_scroll(&hits);
        assert_eq!(
            app.command_palette().first_visible(),
            first,
            "não volta ao topo"
        );
    }

    #[test]
    fn test_toggle_theme_applies_next_theme() {
        let mut app = AppState::new(40, 5).unwrap();
        app.config.colors.name = "dark".into();
        app.config_dirty = false;
        assert_eq!(app.toggle_theme(), "light");
        assert!(app.config_dirty);
        assert_eq!(app.toggle_theme(), "dracula");
    }

    // Linux: o diretório de cada painel vem do /proc (no macOS só pelo
    // OSC 7 do shell, que o `sh -c` do teste não manda).
    #[cfg(target_os = "linux")]
    #[test]
    fn test_workspace_round_trip() {
        let dirs: Vec<tempfile::TempDir> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
        let mut app = AppState::with_command(
            Config::default(),
            81,
            24,
            "/bin/sh",
            &[
                "-c".into(),
                format!("cd {} && sleep 30", dirs[0].path().display()),
            ],
        )
        .unwrap();
        // Os processos dos painéis ficam parados em pastas conhecidas.
        app.split_active_pane_running(
            SplitDirection::Horizontal,
            "/bin/sh",
            &[
                "-c".into(),
                format!("cd {} && exec sleep 30", dirs[1].path().display()),
            ],
        )
        .unwrap();
        app.resize_active_pane(true);
        app.open_tab_running(
            "/bin/sh",
            &[
                "-c".into(),
                format!("cd {} && exec sleep 30", dirs[2].path().display()),
            ],
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        // Título fixo e grupo (recolhido) também voltam.
        app.rename_tab(1, Some("logs"));
        // Cores da tab (#24) voltam com o workspace.
        app.set_tab_color(
            0,
            crate::ui::tab_colors::ColorTarget::Tab,
            Some([255, 128, 0]),
        );
        app.set_tab_color(
            1,
            crate::ui::tab_colors::ColorTarget::Background,
            Some([16, 24, 32]),
        );
        assert!(app.nest_tab(1, 0));
        assert!(app.toggle_tab_group(0));

        let snapshot = app.capture_workspace();
        assert_eq!(snapshot.tabs.len(), 2);
        assert_eq!(snapshot.tabs[1].title.as_deref(), Some("logs"));
        assert_eq!(snapshot.tabs[1].parent, Some(0));
        assert!(snapshot.tabs[0].collapsed);
        let json = serde_json::to_string(&snapshot).unwrap();
        let snapshot: WorkspaceSnapshot = serde_json::from_str(&json).unwrap();

        let mut fresh = AppState::with_command(
            Config::default(),
            81,
            24,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        fresh.restore_workspace(&snapshot).unwrap();
        assert_eq!(fresh.tab_count(), 2);
        assert_eq!(fresh.tab_manager.tabs()[1].title(), "logs");
        assert_eq!(fresh.tab_manager.tabs()[0].color, Some([255, 128, 0]));
        assert_eq!(fresh.tab_manager.tabs()[1].background, Some([16, 24, 32]));
        assert_eq!(fresh.tab_manager.tabs()[1].color, None);
        assert_eq!(fresh.tab_manager.parent_index(1), Some(0));
        assert!(fresh.tab_manager.tabs()[0].is_collapsed());
        let tab = &fresh.tab_manager.tabs()[0];
        assert_eq!(tab.pane_count(), 2);
        let widths: Vec<u16> = tab
            .layout()
            .rects(fresh.pane_area())
            .iter()
            .map(|(_, r)| r.cols)
            .collect();
        let original: Vec<u16> = app.tab_manager.tabs()[0]
            .layout()
            .rects(app.pane_area())
            .iter()
            .map(|(_, r)| r.cols)
            .collect();
        assert_eq!(widths, original);

        // Cada painel restaurado abre no diretório salvo.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let canon = |p: &std::path::Path| p.canonicalize().unwrap();
        let mut cwds = Vec::new();
        while std::time::Instant::now() < deadline {
            cwds = fresh.tab_manager.tabs()[0]
                .pane_ids()
                .into_iter()
                .filter_map(|id| {
                    fresh.tab_manager.tabs()[0]
                        .pane_buffer(id)
                        .and_then(buffer_cwd)
                })
                .map(|p| canon(&p))
                .collect();
            if cwds.len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        cwds.sort();
        let mut expected = vec![canon(dirs[0].path()), canon(dirs[1].path())];
        expected.sort();
        assert_eq!(cwds, expected);
    }

    /// Digitar `usuário@host:porta` sem host salvo preenche os campos ao confirmar.
    #[test]
    fn test_quick_connect_parses_typed_connection_string() {
        use crate::ssh::QuickConnectField;
        let mut ui = QuickConnectUI::default();
        ui.show();
        for c in "deploy@example.org:2222".chars() {
            ui.type_char(c);
        }
        if ui.has_results() {
            return; // há hosts salvos parecidos neste ambiente
        }
        let params = prepare_quick_connect(&mut ui, false).validate().unwrap();
        assert_eq!(ui.get_field(QuickConnectField::Host), "example.org");
        assert_eq!(ui.get_field(QuickConnectField::User), "deploy");
        assert_eq!(params.port, 2222);
    }

    /// Com a IA desligada o painel explica como ligar; o campo de sugestão
    /// aceita texto e Esc fecha.
    #[cfg(unix)]
    #[test]
    fn test_assistant_flow_without_ai_configured() {
        let mut app = AppState::with_command(
            Config::default(),
            80,
            10,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        assert!(app.assistant_view().is_none());
        app.ai_explain_output();
        assert!(
            matches!(app.assistant_view(), Some(AssistantView::Error(msg)) if msg.contains("enabled = true"))
        );
        assert!(app.assistant_key(AssistantKey::Escape));
        assert!(app.assistant_view().is_none());

        app.ai_open_suggest();
        for c in "list files".chars() {
            app.assistant_key(AssistantKey::Char(c));
        }
        app.assistant_key(AssistantKey::Backspace);
        assert_eq!(
            app.assistant_view(),
            Some(&AssistantView::Prompt("list file".into()))
        );
        app.assistant_key(AssistantKey::Enter);
        assert!(matches!(
            app.assistant_view(),
            Some(AssistantView::Error(_))
        ));
        assert!(!app.assistant_key(AssistantKey::Escape) || app.assistant_view().is_none());
    }

    #[cfg(unix)]
    /// Fluxo completo contra um servidor local no formato do Ollama: o pedido
    /// sai em segundo plano, a resposta vira sugestão e Enter a insere no
    /// terminal (sem executar).
    /// Lê um pedido HTTP inteiro (cabeçalhos e corpo pelo Content-Length).
    fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        use std::io::Read;
        let mut request = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                return request;
            }
            request.extend_from_slice(&buf[..n]);
            if http_request_complete(&request) {
                return request;
            }
        }
    }

    #[cfg(unix)]
    fn http_request_complete(request: &[u8]) -> bool {
        let text = String::from_utf8_lossy(request);
        let Some(end) = text.find("\r\n\r\n") else {
            return false;
        };
        let length = text[..end]
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        request.len() >= end + 4 + length
    }

    #[cfg(unix)]
    /// Servidor HTTP local que responde uma vez com `body` (JSON).
    fn serve_once(body: &'static str) -> std::net::SocketAddr {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_http_request(&mut stream);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        addr
    }

    /// Fluxo completo contra um servidor local no formato do Ollama: o pedido
    /// sai em segundo plano, a resposta vira sugestão e Enter a insere no
    /// terminal (sem executar).
    #[cfg(unix)]
    #[test]
    fn test_assistant_suggest_round_trip_inserts_command() {
        let addr = serve_once(r#"{"message":{"content":"```sh\ndu -sh *\n```\nShows sizes."}}"#);
        let mut config = Config::default();
        config.ai.enabled = true;
        config.ai.provider = crate::config::ai::AIProvider::Ollama;
        config.ai.model.name = "test".into();
        config.ai.model.base_url = Some(format!("http://{addr}/api"));
        let mut app = AppState::with_command(config, 60, 6, "/bin/cat", &[]).unwrap();
        app.ai_open_suggest();
        for c in "sizes".chars() {
            app.assistant_key(AssistantKey::Char(c));
        }
        app.assistant_key(AssistantKey::Enter);
        assert_eq!(
            app.assistant_view(),
            Some(&AssistantView::Waiting("Suggest command"))
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while matches!(app.assistant_view(), Some(AssistantView::Waiting(_)))
            && std::time::Instant::now() < deadline
        {
            app.process_output();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        match app.assistant_view() {
            Some(AssistantView::Answer { command, .. }) => {
                assert_eq!(command.as_deref(), Some("du -sh *"))
            }
            other => panic!("{other:?}"),
        }
        app.assistant_key(AssistantKey::Enter);
        assert!(app.assistant_view().is_none());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut screen = String::new();
        while !screen.contains("du -sh *") && std::time::Instant::now() < deadline {
            app.process_output();
            screen = grid_to_string(app.active_tab_grid().unwrap());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(screen.contains("du -sh *"), "{screen}");
    }

    #[cfg(unix)]
    #[test]
    fn test_close_exited_tabs() {
        let mut app = AppState::new(80, 24).unwrap();
        let config = Config::default();
        let short = TerminalBuffer::with_command(
            80,
            24,
            "/bin/sh",
            &["-c".into(), "exit 0".into()],
            &config,
        )
        .unwrap();
        app.tab_manager.create_tab_with_buffer("sh", short);
        assert_eq!(app.tab_count(), 2);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.tab_count() == 2 && std::time::Instant::now() < deadline {
            assert!(!app.close_exited_tabs());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            app.tab_count(),
            1,
            "a tab do shell que saiu continuou aberta"
        );

        // Fechar a última tab avisa que a janela deve fechar.
        let mut lone = AppState::new(80, 24).unwrap();
        lone.tab_manager.close_tab(0);
        let done = TerminalBuffer::with_command(
            80,
            24,
            "/bin/sh",
            &["-c".into(), "exit 0".into()],
            &config,
        )
        .unwrap();
        lone.tab_manager.create_tab_with_buffer("sh", done);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut should_close = false;
        while !should_close && std::time::Instant::now() < deadline {
            should_close = lone.close_exited_tabs();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(should_close);
    }

    #[test]
    fn test_app_state_creation() {
        let app = AppState::new(80, 24);
        assert!(app.is_ok());

        let app = app.unwrap();
        assert_eq!(app.tab_manager.tab_count(), 1);
        assert_eq!(app.terminal_size, (80, 24));
    }

    #[test]
    fn test_create_new_tab() {
        let mut app = AppState::new(80, 24).unwrap();
        assert_eq!(app.tab_manager.tab_count(), 1);

        app.create_new_tab().unwrap();
        assert_eq!(app.tab_manager.tab_count(), 2);
    }

    #[test]
    fn test_go_to_tab() {
        let mut app = AppState::new(80, 24).unwrap();
        app.create_new_tab().unwrap();
        app.create_new_tab().unwrap();

        app.go_to_tab(2);
        assert_eq!(app.tab_manager.active_tab_index(), 2);

        app.go_to_tab(0);
        assert_eq!(app.tab_manager.active_tab_index(), 0);
    }

    #[test]
    fn test_next_previous_tab() {
        let mut app = AppState::new(80, 24).unwrap();
        app.create_new_tab().unwrap();
        app.create_new_tab().unwrap();

        app.next_tab();
        assert_eq!(app.tab_manager.active_tab_index(), 0);

        app.previous_tab();
        assert_eq!(app.tab_manager.active_tab_index(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn test_close_tab_confirms_when_config_enabled() {
        let mut app = AppState::new(80, 24).unwrap();

        // Com confirmação habilitada (padrão)
        assert!(app.config.window.confirm_close_tab);

        // Cria uma segunda tab, com um programa em primeiro plano.
        app.open_tab_running("/bin/bash", &["--norc".into(), "-i".into()])
            .unwrap();
        app.go_to_tab(1);
        app.write_to_active_tab(b"sleep 30\n").unwrap();
        assert_eq!(app.tab_count(), 2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.active_tab().unwrap().busy_process().is_none()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Com processo rodando e confirmação habilitada, fica pendente.
        let should_exit = app.close_active_tab();
        assert!(!should_exit);
        assert_eq!(app.tab_count(), 2);
        assert_eq!(app.pending_close_tab_index(), Some(1));

        let should_exit = app.confirm_close_pending_tab();
        assert!(!should_exit); // Ainda há uma tab
        assert_eq!(app.tab_count(), 1);
    }

    /// Regressão: o shell parado contava como processo rodando e o
    /// Ctrl+Shift+W nunca fechava a tab (a confirmação não tinha tela).
    #[cfg(unix)]
    #[test]
    fn test_close_tab_with_idle_shell_closes_without_confirmation() {
        let mut app = AppState::new(80, 24).unwrap();
        app.open_tab_running("/bin/bash", &["--norc".into(), "-i".into()])
            .unwrap();
        app.go_to_tab(1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.active_tab().unwrap().busy_process().is_some()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!app.close_active_tab());
        assert_eq!(app.tab_count(), 1);
        assert_eq!(app.pending_close_tab_index(), None);
    }

    #[test]
    fn test_close_tab_skips_confirm_when_config_disabled() {
        let mut app = AppState::new(80, 24).unwrap();

        // Cria uma segunda tab
        app.create_new_tab().unwrap();
        assert_eq!(app.tab_count(), 2);

        // Desabilita confirmação
        app.config.window.confirm_close_tab = false;

        // Deve fechar sem pedir confirmação
        let should_exit = app.close_active_tab();
        assert!(!should_exit); // Ainda há uma tab
        assert_eq!(app.tab_count(), 1);
    }

    #[test]
    fn test_pending_close_tab() {
        let mut app = AppState::new(80, 24).unwrap();

        // Cria uma segunda tab para não sair da aplicação quando confirmar
        app.create_new_tab().unwrap();
        assert_eq!(app.tab_count(), 2);

        // Sem pendência inicialmente
        assert!(!app.has_pending_close_tab());
        assert_eq!(app.pending_close_tab_index(), None);

        // Simula uma situação onde precisaria de confirmação
        app.pending_close_tab = Some(0);

        assert!(app.has_pending_close_tab());
        assert_eq!(app.pending_close_tab_index(), Some(0));

        // Cancela
        app.cancel_close_pending_tab();
        assert!(!app.has_pending_close_tab());

        // Simula novamente e confirma
        app.pending_close_tab = Some(0);
        let should_exit = app.confirm_close_pending_tab();
        assert!(!app.has_pending_close_tab());
        assert!(!should_exit); // Ainda há uma tab
        assert_eq!(app.tab_count(), 1);
    }

    // === Quick Connect Tests ===

    #[test]
    fn test_selection_worth_copying_ignores_plain_click() {
        use crate::terminal::grid::{Selection, SelectionKind};
        let click = Selection {
            start: (2, 5),
            end: (2, 5),
            active: true,
            kind: SelectionKind::Normal,
        };
        assert!(!selection_worth_copying(&click));
        let drag = Selection {
            end: (2, 9),
            ..click
        };
        assert!(selection_worth_copying(&drag));
        let word = Selection {
            kind: SelectionKind::Word,
            ..click
        };
        assert!(selection_worth_copying(&word));
        let line = Selection {
            kind: SelectionKind::Line,
            ..click
        };
        assert!(selection_worth_copying(&line));
        let inactive = Selection {
            active: false,
            ..drag
        };
        assert!(!selection_worth_copying(&inactive));
    }

    #[test]
    fn test_copy_on_select_without_selection_copies_nothing() {
        let mut app = AppState::new(80, 24).unwrap();
        assert_eq!(app.copy_on_select(), None);
        assert!(app.toast().is_none());
        // Só o clique: o caractere sob o cursor fica marcado, mas não copia.
        if let Some(grid) = app.active_tab_mut().and_then(|t| t.grid_mut()) {
            grid.start_selection(1, 1, crate::terminal::grid::SelectionKind::Normal);
        }
        assert_eq!(app.copy_on_select(), None);
        assert!(app.toast().is_none());
    }

    #[test]
    fn test_copy_on_select_respects_config() {
        let mut app = AppState::new(80, 24).unwrap();
        app.config.terminal.copy_on_select = false;
        if let Some(grid) = app.active_tab_mut().and_then(|t| t.grid_mut()) {
            grid.select_line_at(0);
        }
        assert_eq!(app.copy_on_select(), None);
        assert!(app.toast().is_none());
    }

    #[test]
    fn test_show_toast_is_visible_right_away() {
        let mut app = AppState::new(80, 24).unwrap();
        assert!(app.toast().is_none());
        app.show_toast("Copiado · 3 caracteres", crate::ui::toast::ToastKind::Info);
        assert_eq!(
            app.toast().map(|t| t.text.as_str()),
            Some("Copiado · 3 caracteres")
        );
    }

    #[test]
    #[ignore = "Modifies system clipboard - run manually with: cargo test -- --ignored"]
    fn test_copy_on_select_copies_line_and_shows_toast() {
        let mut app = AppState::new(80, 24).unwrap();
        if let Some(grid) = app.active_tab_mut().and_then(|t| t.grid_mut()) {
            grid.select_line_at(0);
        }
        let copied = app.copy_on_select();
        assert!(copied.is_some());
        assert!(app.toast().is_some_and(|t| t.text.starts_with("Cop")));
    }

    #[test]
    fn test_quick_connect_initially_hidden() {
        let app = AppState::new(80, 24).unwrap();
        assert!(!app.is_quick_connect_visible());
        assert!(app.quick_connect_ui().is_none());
    }

    #[test]
    fn test_quick_connect_show_hide() {
        let mut app = AppState::new(80, 24).unwrap();

        // Inicialmente escondido
        assert!(!app.is_quick_connect_visible());

        // Mostra o Quick Connect
        app.show_quick_connect();
        assert!(app.is_quick_connect_visible());
        assert!(app.quick_connect_ui().is_some());

        // Esconde
        app.hide_quick_connect();
        assert!(!app.is_quick_connect_visible());
    }

    #[test]
    fn test_quick_connect_toggle() {
        let mut app = AppState::new(80, 24).unwrap();

        // Toggle para mostrar
        app.toggle_quick_connect();
        assert!(app.is_quick_connect_visible());

        // Toggle para esconder
        app.toggle_quick_connect();
        assert!(!app.is_quick_connect_visible());
    }

    #[test]
    fn test_quick_connect_handle_key() {
        let mut app = AppState::new(80, 24).unwrap();
        app.show_quick_connect();

        // Teclas devem ser consumidas quando o Quick Connect está visível
        assert!(app.handle_quick_connect_key(QuickConnectKey::Char('t')));
        assert!(app.handle_quick_connect_key(QuickConnectKey::Char('e')));
        assert!(app.handle_quick_connect_key(QuickConnectKey::Char('s')));
        assert!(app.handle_quick_connect_key(QuickConnectKey::Char('t')));

        // Verifica se o texto foi digitado
        if let Some(ui) = app.quick_connect_ui() {
            assert_eq!(ui.get_field(crate::ssh::QuickConnectField::Host), "test");
        }

        // Esconde
        app.hide_quick_connect();

        // Agora as teclas não devem ser consumidas
        assert!(!app.handle_quick_connect_key(QuickConnectKey::Char('x')));
    }

    #[test]
    fn test_quick_connect_fill() {
        let mut app = AppState::new(80, 24).unwrap();
        app.show_quick_connect();

        // Preenche com uma string de conexão
        app.quick_connect_fill("admin@example.com:2222");

        // Verifica se os campos foram preenchidos
        if let Some(ui) = app.quick_connect_ui() {
            assert_eq!(
                ui.get_field(crate::ssh::QuickConnectField::Host),
                "example.com"
            );
            assert_eq!(ui.get_field(crate::ssh::QuickConnectField::User), "admin");
            assert_eq!(ui.get_field(crate::ssh::QuickConnectField::Port), "2222");
        }
    }

    #[test]
    fn test_ssh_manager_access() {
        let app = AppState::new(80, 24).unwrap();

        // Deve retornar uma referência ao gerenciador SSH
        let manager = app.ssh_manager();
        drop(manager.read());
    }

    fn write_to_active_grid(app: &mut AppState, data: &[u8]) {
        let grid = app
            .active_tab_mut()
            .and_then(|tab| tab.grid_mut())
            .expect("grid da tab ativa");
        crate::terminal::parser::Parser::new(grid).process(data);
    }

    /// AppState sem nenhuma tab, para exercitar os caminhos "sem tab ativa".
    fn app_without_tabs() -> AppState {
        let mut app = AppState::new(80, 24).unwrap();
        app.tab_manager = TabManager::new();
        app
    }

    #[test]
    fn test_grid_to_string_trims_trailing_spaces_and_last_newline() {
        let mut grid = crate::terminal::grid::Grid::new(10, 3);
        crate::terminal::parser::Parser::new(&mut grid).process(b"ab  \r\n\r\n x");
        assert_eq!(grid_to_string(&grid), "ab\n\n x");

        let empty = crate::terminal::grid::Grid::new(4, 2);
        assert_eq!(grid_to_string(&empty), "\n");
    }

    #[test]
    fn test_active_tab_accessors() {
        let mut app = AppState::new(80, 24).unwrap();
        app.create_new_tab().unwrap();
        assert_eq!(app.active_tab_index(), 1);
        assert!(app.active_tab_mut().is_some());
        let grid = app.active_tab_grid().expect("grid");
        assert_eq!(grid.size(), (80, 24));

        let mut app = app_without_tabs();
        assert!(app.active_tab_mut().is_none());
        assert!(app.active_tab_grid().is_none());
        assert!(!app.has_selection());
        // Sem tab ativa a escrita é ignorada sem erro
        assert!(app.write_to_active_tab(b"ls\r").is_ok());
        app.process_active_tab_output();
        app.select_all();
        app.clear_selection();
    }

    #[test]
    fn test_write_and_process_output_on_real_tabs() {
        let mut app = AppState::new(80, 24).unwrap();
        app.create_new_tab().unwrap();
        // Tab sem buffer também deve ser tolerada
        app.tab_manager.create_tab("sem-buffer");
        assert!(app.write_to_active_tab(b"echo oi\r").is_ok());
        app.process_output();
        app.process_active_tab_output();
        assert_eq!(app.tab_manager.tab_count(), 3);
    }

    #[test]
    fn test_resize_updates_size_of_every_buffer() {
        let mut app = AppState::new(80, 24).unwrap();
        app.create_new_tab().unwrap();
        app.tab_manager.create_tab("sem-buffer");
        app.resize(100, 30).unwrap();
        assert_eq!(app.terminal_size, (100, 30));
        for index in 0..2 {
            let tab = app.tab_manager.get_tab(index).expect("tab");
            assert_eq!(tab.grid().expect("grid").size(), (100, 30));
        }
    }

    #[test]
    fn test_close_tab_returns_true_when_last_tab_closes() {
        let mut app = app_without_tabs();
        assert!(app.close_tab(0), "sem tabs deve sinalizar saída");

        app.tab_manager.create_tab("a");
        app.tab_manager.create_tab("b");
        // Tabs sem processo fecham direto
        assert!(!app.close_tab(1));
        assert_eq!(app.tab_manager.tab_count(), 1);
        assert!(app.close_active_tab());
        assert_eq!(app.tab_manager.tab_count(), 0);
    }

    #[test]
    fn test_confirm_close_without_pending_is_noop() {
        let mut app = AppState::new(80, 24).unwrap();
        assert!(!app.confirm_close_pending_tab());
        assert_eq!(app.tab_manager.tab_count(), 1);
    }

    #[test]
    fn test_clipboard_operations_fail_without_clipboard() {
        let mut app = AppState::new(80, 24).unwrap();
        // Em ambiente sem display o clipboard pode não existir; forçamos o caso.
        app.clipboard = None;
        assert!(!app.clipboard_available());
        for result in [
            app.copy_visible_content(),
            app.copy_text("x"),
            app.copy_selection(),
            app.paste_from_clipboard(),
        ] {
            let error = result.expect_err("sem clipboard deve falhar");
            assert_eq!(error.to_string(), "Clipboard not available");
        }
    }

    /// #34: com histórico, o destaque e o "próximo" caem na linha certa
    /// (numeração do grid: histórico + tela), inclusive no histórico.
    #[test]
    fn test_search_uses_absolute_lines_with_history() {
        let mut app = AppState::new(80, 24).unwrap();
        let mut text = String::new();
        for i in 0..500 {
            text.push_str(&format!("linha {i:03}\r\n"));
        }
        write_to_active_grid(&mut app, text.as_bytes());
        let (history, rows) = {
            let grid = app.active_tab().unwrap().grid().unwrap();
            (grid.scrollback_len(), grid.rows())
        };
        assert!(history > 400, "{history}");
        // Rola até o meio do histórico.
        let grid = app.active_tab_mut().unwrap().grid_mut().unwrap();
        grid.scroll_to_line(250);
        let top = history - grid.scroll_offset();
        assert_eq!(top, 250);
        // Texto visível: "linha 255" está na linha absoluta 255.
        app.update_search_query("linha 255");
        assert_eq!(app.get_current_match_position(), Some((255, 0, 9)));
        let grid = app.active_tab().unwrap().grid().unwrap();
        assert!(grid.is_highlighted(255, 0));
        assert!(!grid.is_highlighted(5, 0), "linha antiga sem destaque");
        let visible = grid.visible_lines();
        let row = 255 - top;
        assert!(visible[row].iter().take(9).all(|cell| cell.highlight));
        // Vários resultados: começa no primeiro da tela e "próximo" segue
        // pela numeração, rolando quando sai da tela.
        app.update_search_query("linha 2");
        assert_eq!(
            app.get_current_match_position().unwrap().0,
            250,
            "primeiro resultado visível ({top}..{})",
            top + rows
        );
        assert!(app.next_search_match());
        assert_eq!(app.get_current_match_position().unwrap().0, 251);
        // Para trás de onde começou: linha 249, acima da tela, rolando.
        assert!(app.previous_search_match());
        assert!(app.previous_search_match());
        assert_eq!(app.get_current_match_position().unwrap().0, 249);
        let grid = app.active_tab().unwrap().grid().unwrap();
        let top = grid.scrollback_len() - grid.scroll_offset();
        assert!((top..top + rows).contains(&249), "rolou até a linha 249");
        // Resultado na tela (fora do histórico): volta para o fim.
        app.update_search_query("linha 499");
        let line = app.get_current_match_position().unwrap().0;
        assert!(line >= history, "{line} na tela");
        app.active_tab_mut()
            .unwrap()
            .grid_mut()
            .unwrap()
            .scroll_to_line(10);
        assert!(app.next_search_match());
        assert_eq!(app.active_tab().unwrap().grid().unwrap().scroll_offset(), 0);
    }

    #[test]
    fn test_search_delegates_to_tab_manager() {
        let mut app = AppState::new(80, 24).unwrap();
        write_to_active_grid(&mut app, b"foo bar foo");

        assert!(!app.is_search_visible());
        app.toggle_search();
        assert!(app.is_search_visible());
        app.toggle_search();
        assert!(!app.is_search_visible());

        app.start_search();
        assert!(app.is_search_visible());
        app.update_search_query("foo");
        assert_eq!(app.get_search_query(), "foo");
        assert!(app.is_search_active());
        assert_eq!(app.get_current_match_position(), Some((0, 0, 3)));
        assert!(app.next_search_match());
        assert_eq!(app.get_current_match_position(), Some((0, 8, 11)));
        assert!(app.previous_search_match());
        assert_eq!(app.get_current_match_position(), Some((0, 0, 3)));
        assert!(
            app.get_search_status().contains('2'),
            "{}",
            app.get_search_status()
        );

        app.toggle_search_case_sensitive();
        app.toggle_search_whole_word();
        app.toggle_search_regex();
        assert_eq!(app.get_search_status(), app.tab_manager.get_search_status());

        app.stop_search();
        assert!(!app.is_search_visible());
    }

    #[test]
    fn test_selection_lifecycle() {
        use crate::ui::mouse_selection::{MouseButton, MouseSelectionEvent};

        let mut app = AppState::new(20, 3).unwrap();
        write_to_active_grid(&mut app, b"hello world");
        assert!(!app.has_selection());

        app.select_all();
        assert!(app.has_selection());
        app.clear_selection();
        assert!(!app.has_selection());
        assert!(!app.mouse_selection_mut().has_selection());

        app.handle_mouse_selection_event(MouseSelectionEvent::Press {
            row: 0,
            col: 0,
            button: MouseButton::Left,
            alt: false,
        });
        app.handle_mouse_selection_event(MouseSelectionEvent::Drag {
            row: 0,
            col: 4,
            alt: false,
        });
        app.handle_mouse_selection_event(MouseSelectionEvent::Release {
            row: 0,
            col: 4,
            button: MouseButton::Left,
        });
        assert!(app.has_selection());
        let text = app.active_tab_grid().expect("grid").get_selected_text();
        assert!(text.starts_with("hell"), "{text}");

        // Sem tab ativa o evento é ignorado
        let mut empty = app_without_tabs();
        empty.handle_mouse_selection_event(MouseSelectionEvent::Press {
            row: 0,
            col: 0,
            button: MouseButton::Left,
            alt: false,
        });
        assert!(!empty.mouse_selection_mut().is_selecting());
    }

    #[test]
    fn test_create_ssh_tab_opens_active_tab_with_connection_title() {
        let mut app = AppState::new(80, 24).unwrap();
        // Porta 1 no loopback: conexão recusada, sem rede externa.
        let params = QuickConnectParams {
            hostname: "127.0.0.1".to_string(),
            port: 1,
            user: Some("ci".to_string()),
            identity_file: None,
        };
        app.create_ssh_tab(&params).unwrap();
        assert_eq!(app.tab_manager.tab_count(), 2);
        assert_eq!(app.active_tab_index(), 1);
        let tab = app.tab_manager.get_tab(1).expect("tab ssh");
        assert!(tab.title().contains("ci@127.0.0.1:1"), "{}", tab.title());
    }

    #[test]
    fn test_quick_connect_enter_validates_and_connects() {
        let mut app = AppState::new(80, 24).unwrap();

        // Enter sem diálogo aberto não é consumido
        assert!(!app.handle_quick_connect_key(QuickConnectKey::Enter));
        assert!(app.quick_connect_ui_mut().is_none());

        app.show_quick_connect();
        // Host vazio: validação falha e a mensagem aparece no diálogo
        assert!(app.handle_quick_connect_key(QuickConnectKey::Enter));
        let ui = app.quick_connect_ui().expect("ui");
        assert!(ui.is_visible());
        assert!(ui.error_message().is_some());
        assert_eq!(app.tab_manager.tab_count(), 1);

        app.quick_connect_fill("ci@127.0.0.1:1");
        assert!(app.handle_quick_connect_key(QuickConnectKey::Enter));
        assert_eq!(app.tab_manager.tab_count(), 2);
        assert!(!app.is_quick_connect_visible());
        let recent = app.ssh_manager().read().get_recent_connections();
        assert!(!recent.is_empty());

        // Diálogo escondido: Enter volta a não ser consumido
        assert!(!app.handle_quick_connect_key(QuickConnectKey::Enter));
    }

    #[test]
    fn test_command_palette_keys() {
        use crate::ui::command_palette::PaletteInputKey;

        let mut app = AppState::new(80, 24).unwrap();
        assert!(!app.is_command_palette_visible());
        app.toggle_command_palette();
        assert!(app.is_command_palette_visible());
        app.hide_command_palette();
        assert!(!app.is_command_palette_visible());
        app.show_command_palette();

        let first = app.command_palette().selected_item().cloned();
        assert_eq!(
            app.handle_command_palette_key(&PaletteInputKey::Down),
            PaletteAction::None
        );
        assert_ne!(app.command_palette().selected_item().cloned(), first);
        app.handle_command_palette_key(&PaletteInputKey::Up);
        assert_eq!(app.command_palette().selected_item().cloned(), first);

        for c in "new tabx".chars() {
            app.handle_command_palette_key(&PaletteInputKey::Char(c));
        }
        assert!(app.command_palette().items().is_empty());
        app.handle_command_palette_key(&PaletteInputKey::Backspace);
        assert_eq!(app.command_palette().items().len(), 1);
        assert_eq!(
            app.handle_command_palette_key(&PaletteInputKey::Enter),
            PaletteAction::NewTab
        );
        assert!(!app.is_command_palette_visible());

        app.command_palette_mut().open();
        assert_eq!(
            app.handle_command_palette_key(&PaletteInputKey::Escape),
            PaletteAction::None
        );
        assert!(!app.is_command_palette_visible());
    }

    #[test]
    fn test_onboarding_accessors_agree() {
        let app = AppState::new(80, 24).unwrap();
        assert_eq!(app.is_onboarding_visible(), app.onboarding().is_visible());
    }
}
