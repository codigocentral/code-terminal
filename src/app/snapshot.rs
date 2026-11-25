//! `--snapshot`: roda um comando num PTY de verdade, desenha o frame na GPU
//! sem abrir janela e salva um PNG.
//!
//! Serve para ver o terminal sem monitor (CI, sessões remotas) e para testes
//! visuais. Exemplo:
//!
//! ```text
//! code-terminal --snapshot cores.png --size 900x600 -- bash -c 'ls --color=always'
//! ```

use crate::app::state::AppState;
use crate::config::theme::Theme;
use crate::config::Config;
use crate::renderer::gpu::GpuState;
use crate::renderer::GridLayout;
use crate::ui::pane_layout::SplitDirection;
use crate::ui::window::compose_frame;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Painel aberto por cima do terminal no snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOverlay {
    Onboarding,
    Palette,
    /// "Choose Theme…": a lista de temas da paleta.
    Themes,
    Settings,
    Search,
    Notifications,
    Inspector,
    Ssh,
    Ai,
    /// Menu de contexto (botão direito) da tab ativa na sidebar.
    TabMenu,
    /// Título da tab ativa em edição na sidebar.
    Rename,
    /// Pergunta ao fechar a tab ativa (persistente: encerrar ou esconder).
    Close,
}

impl SnapshotOverlay {
    fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "onboarding" => Self::Onboarding,
            "palette" => Self::Palette,
            "themes" => Self::Themes,
            "settings" => Self::Settings,
            "search" => Self::Search,
            "notifications" => Self::Notifications,
            "inspector" => Self::Inspector,
            "ssh" => Self::Ssh,
            "ai" => Self::Ai,
            "tab-menu" => Self::TabMenu,
            "rename" => Self::Rename,
            "close" => Self::Close,
            other => bail!(
                "unknown overlay '{other}' (onboarding, palette, themes, settings, search, notifications, inspector, ssh, ai, tab-menu, rename, close)"
            ),
        })
    }
}

/// Opções do `--snapshot`.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotOptions {
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Programa e argumentos; vazio roda o shell interativo.
    pub command: Vec<String>,
    /// Tempo máximo esperando saída; termina antes se o comando sair.
    pub wait: Duration,
    pub overlay: Option<SnapshotOverlay>,
    pub theme: Option<String>,
    pub font_size: Option<f32>,
    /// Painéis extras: direção e comando (rodado com `sh -c`).
    pub splits: Vec<(SplitDirection, String)>,
    /// Tabs extras, cada uma rodando um comando com `sh -c`.
    pub tabs: Vec<String>,
    /// Pasta de plugins WASM a carregar (como `<config>/plugins`).
    pub plugins: Option<PathBuf>,
    /// Títulos fixos: índice da tab e título.
    pub renames: Vec<(usize, String)>,
    /// Grupos: tab e tab de cima, aplicados em ordem.
    pub nests: Vec<(usize, usize)>,
    /// Grupos recolhidos (índice da tab de cima).
    pub collapse: Vec<usize>,
    /// Tab ativa no print.
    pub active: Option<usize>,
    /// Tabs tornadas persistentes (o shell passa para o daemon).
    pub persist: Vec<usize>,
    /// Trazer as abas persistentes soltas (ou de antes do reboot), como a
    /// janela faz ao abrir.
    pub attach_persistent: bool,
    /// Arraste em andamento: tab e altura (pixels) onde o mouse está.
    pub drag: Option<(usize, f32)>,
    /// Mouse parado em (x, y): mostra os botões que aparecem ao passar o mouse.
    pub hover: Option<(f32, f32)>,
    /// Seção aberta na tela de configurações (`--overlay settings`).
    pub section: Option<usize>,
    /// Teclas para a tela de configurações, separadas por vírgula (`down`,
    /// `enter`, `esc`, `tab`, `bs` ou um caractere): mostra estados como o
    /// formulário de host SSH aberto.
    pub settings_keys: Vec<crate::ui::settings::SettingsInputKey>,
    /// Abre e fecha a tela de configurações N vezes (desenhando) e mede a
    /// memória privada antes e depois: a tela não pode ficar alocada.
    pub settings_cycles: u32,
    /// Cores de tab (#24): índice, alvo e cor.
    pub tab_colors: Vec<(usize, crate::ui::tab_colors::ColorTarget, [u8; 3])>,
    /// Aviso não lido na tab ativa (a animação do aviso aparece no print).
    pub notice: bool,
    /// Desenha o frame mais N vezes e mede a CPU por frame (custo de animar).
    pub frames: u32,
}

/// Tempos medidos durante o snapshot.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotReport {
    pub gpu: Duration,
    pub output: Duration,
    pub render: Duration,
    pub cols: u16,
    pub rows: u16,
    /// Com `--settings-cycles`: memória privada (kB) antes, depois de 10
    /// ciclos e depois de todos.
    pub settings_memory: Option<(u64, u64, u64)>,
    /// Com `--frames`: CPU e tempo médios por frame redesenhado.
    pub per_frame: Option<(Duration, Duration)>,
}

/// `down,enter,a,b` → teclas da tela de configurações.
fn parse_settings_keys(raw: &str) -> Result<Vec<crate::ui::settings::SettingsInputKey>> {
    use crate::ui::settings::SettingsInputKey as K;
    raw.split(',')
        .filter(|token| !token.is_empty())
        .map(|token| {
            Ok(match token {
                "up" => K::Up,
                "down" => K::Down,
                "left" => K::Left,
                "right" => K::Right,
                "enter" => K::Enter,
                "esc" => K::Escape,
                "tab" => K::NextSection,
                "bs" => K::Backspace,
                "comma" => K::Char(','),
                one if one.chars().count() == 1 => K::Char(one.chars().next().unwrap_or(' ')),
                other => bail!("invalid key '{other}' in --settings-keys"),
            })
        })
        .collect()
}

const USAGE: &str = "usage: code-terminal --snapshot <out.png> [--size WxH] [--wait-ms N] \
[--overlay onboarding|palette|themes|settings|search|notifications|inspector|ssh|ai] [--theme NAME] [--font-size PT] \
[--split-h CMD] [--split-v CMD] [--tab CMD] [--plugins DIR] [--rename I=TITLE] [--nest CHILD:PARENT] \
[--collapse I] [--active I] [--drag I:Y] [--persist I] [--attach-persistent] [--hover X:Y] [--section N] [--settings-keys K,K] [--tab-color I=#RGB] [--tab-bg I=#RGB] [--notice] [--frames N] [-- command args...]";

/// "a:b" (ou "a=b") em dois pedaços.
fn split_pair<'a>(value: &'a str, name: &str, sep: char) -> Result<(&'a str, &'a str)> {
    value
        .split_once(sep)
        .with_context(|| format!("invalid {name} '{value}'\n{USAGE}"))
}

fn parse_index(value: &str, name: &str) -> Result<usize> {
    value
        .trim()
        .parse()
        .with_context(|| format!("invalid tab index in {name} '{value}'"))
}

fn parse_size(value: &str) -> Result<(u32, u32)> {
    let (w, h) = value
        .split_once('x')
        .with_context(|| format!("invalid --size '{value}', expected WxH"))?;
    let w: u32 = w
        .parse()
        .with_context(|| format!("invalid width in '{value}'"))?;
    let h: u32 = h
        .parse()
        .with_context(|| format!("invalid height in '{value}'"))?;
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("--size must be between 1x1 and 8192x8192");
    }
    Ok((w, h))
}

/// Lê os argumentos. `Ok(None)` quando não há `--snapshot` (abre a janela normal).
pub fn parse_args<I>(args: I) -> Result<Option<SnapshotOptions>>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    // Sem `--snapshot` a janela abre normalmente, mesmo com outros argumentos
    // (lançadores às vezes passam opções próprias).
    if !args.iter().any(|arg| arg == "--snapshot") {
        return Ok(None);
    }
    let mut args = args.into_iter();
    let mut output = None;
    let mut opts = SnapshotOptions {
        output: PathBuf::new(),
        width: 900,
        height: 600,
        command: Vec::new(),
        wait: Duration::from_millis(1500),
        overlay: None,
        theme: None,
        font_size: None,
        splits: Vec::new(),
        tabs: Vec::new(),
        plugins: None,
        renames: Vec::new(),
        nests: Vec::new(),
        collapse: Vec::new(),
        active: None,
        persist: Vec::new(),
        attach_persistent: false,
        drag: None,
        hover: None,
        section: None,
        settings_keys: Vec::new(),
        settings_cycles: 0,
        tab_colors: Vec::new(),
        notice: false,
        frames: 0,
    };

    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .with_context(|| format!("{name} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--snapshot" => output = Some(PathBuf::from(value("--snapshot")?)),
            "--size" => (opts.width, opts.height) = parse_size(&value("--size")?)?,
            "--wait-ms" => {
                let ms: u64 = value("--wait-ms")?.parse().context("invalid --wait-ms")?;
                opts.wait = Duration::from_millis(ms);
            }
            "--overlay" => opts.overlay = Some(SnapshotOverlay::parse(&value("--overlay")?)?),
            "--theme" => opts.theme = Some(value("--theme")?),
            "--tab" => opts.tabs.push(value("--tab")?),
            "--plugins" => opts.plugins = Some(PathBuf::from(value("--plugins")?)),
            "--rename" => {
                let raw = value("--rename")?;
                let (index, title) = split_pair(&raw, "--rename", '=')?;
                opts.renames
                    .push((parse_index(index, "--rename")?, title.to_string()));
            }
            "--nest" => {
                let raw = value("--nest")?;
                let (child, parent) = split_pair(&raw, "--nest", ':')?;
                opts.nests.push((
                    parse_index(child, "--nest")?,
                    parse_index(parent, "--nest")?,
                ));
            }
            "--collapse" => opts
                .collapse
                .push(parse_index(&value("--collapse")?, "--collapse")?),
            "--active" => opts.active = Some(parse_index(&value("--active")?, "--active")?),
            "--attach-persistent" => opts.attach_persistent = true,
            "--persist" => opts
                .persist
                .push(parse_index(&value("--persist")?, "--persist")?),
            "--drag" => {
                let raw = value("--drag")?;
                let (index, y) = split_pair(&raw, "--drag", ':')?;
                let y: f32 = y.trim().parse().context("invalid y in --drag")?;
                opts.drag = Some((parse_index(index, "--drag")?, y));
            }
            "--hover" => {
                let raw = value("--hover")?;
                let (x, y) = split_pair(&raw, "--hover", ':')?;
                let x: f32 = x.trim().parse().context("invalid x in --hover")?;
                let y: f32 = y.trim().parse().context("invalid y in --hover")?;
                opts.hover = Some((x, y));
            }
            "--section" => opts.section = Some(parse_index(&value("--section")?, "--section")?),
            "--settings-keys" => {
                opts.settings_keys = parse_settings_keys(&value("--settings-keys")?)?
            }
            "--settings-cycles" => {
                opts.settings_cycles = value("--settings-cycles")?
                    .parse()
                    .context("invalid --settings-cycles")?
            }
            flag @ ("--tab-color" | "--tab-bg") => {
                use crate::ui::tab_colors::{parse_hex, ColorTarget};
                let raw = value(flag)?;
                let (index, hex) = split_pair(&raw, flag, '=')?;
                let color = parse_hex(hex).with_context(|| format!("invalid color in {flag}"))?;
                let target = if flag == "--tab-color" {
                    ColorTarget::Tab
                } else {
                    ColorTarget::Background
                };
                opts.tab_colors
                    .push((parse_index(index, flag)?, target, color));
            }
            "--notice" => opts.notice = true,
            "--frames" => opts.frames = value("--frames")?.parse().context("invalid --frames")?,
            "--split-h" => opts
                .splits
                .push((SplitDirection::Horizontal, value("--split-h")?)),
            "--split-v" => opts
                .splits
                .push((SplitDirection::Vertical, value("--split-v")?)),
            "--font-size" => {
                opts.font_size = Some(
                    value("--font-size")?
                        .parse()
                        .context("invalid --font-size")?,
                )
            }
            "--" => {
                opts.command = args.by_ref().collect();
                break;
            }
            other => bail!("unknown argument '{other}'\n{USAGE}"),
        }
    }

    opts.output = output.with_context(|| format!("--snapshot needs a value\n{USAGE}"))?;
    Ok(Some(opts))
}

/// Programa e argumentos: o comando pedido ou o shell interativo.
fn program_and_args(command: &[String]) -> (String, Vec<String>) {
    match command.split_first() {
        Some((program, args)) => (program.clone(), args.to_vec()),
        None => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            (shell, vec!["-i".to_string()])
        }
    }
}

/// Lê a saída até o comando terminar (e a saída parar) ou até `wait`.
fn pump_output(app: &mut AppState, wait: Duration) {
    let deadline = Instant::now() + wait;
    let quiet_after_exit = Duration::from_millis(100);
    let mut exited_at: Option<Instant> = None;
    while Instant::now() < deadline {
        app.process_output();
        let running = app
            .active_tab()
            .is_some_and(|tab| tab.has_running_process());
        match (running, exited_at) {
            (false, None) => exited_at = Some(Instant::now()),
            (false, Some(at)) if at.elapsed() >= quiet_after_exit => break,
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    app.process_output();
}

fn open_overlay(app: &mut AppState, overlay: Option<SnapshotOverlay>) {
    app.set_onboarding_visible(overlay == Some(SnapshotOverlay::Onboarding));
    match overlay {
        Some(SnapshotOverlay::Palette) => app.toggle_command_palette(),
        Some(SnapshotOverlay::Themes) => app.open_theme_chooser(),
        Some(SnapshotOverlay::Settings) => app.show_settings(),
        Some(SnapshotOverlay::Search) => app.start_search(),
        Some(SnapshotOverlay::Notifications) => app.toggle_notifications_panel(),
        Some(SnapshotOverlay::Inspector) => {}
        Some(SnapshotOverlay::Ssh) => app.show_quick_connect(),
        Some(SnapshotOverlay::Ai) => app.ai_explain_output(),
        Some(SnapshotOverlay::TabMenu) => {
            // Como o botão direito no meio do cartão da tab ativa.
            let row = app
                .tab_manager
                .visible_tabs()
                .iter()
                .position(|&tab| tab == app.active_tab_index())
                .unwrap_or(0);
            let y = crate::renderer::TAB_BAR_HEIGHT as f32
                + (row as f32 + 0.5) * crate::ui::sidebar_rows::SIDEBAR_ENTRY_HEIGHT;
            app.open_tab_menu(app.active_tab_index(), 60.0, y);
        }
        Some(SnapshotOverlay::Rename) => {
            app.start_rename_active_tab();
            app.tab_rename_key(crate::ui::line_edit::LineEditKey::End);
            for c in " (prod)".chars() {
                app.tab_rename_key(crate::ui::line_edit::LineEditKey::Char(c));
            }
        }
        Some(SnapshotOverlay::Close) => {
            app.close_active_tab();
        }
        Some(SnapshotOverlay::Onboarding) | None => {}
    }
}

/// Títulos, grupos, tab ativa e arraste pedidos na linha de comando.
fn apply_sidebar_options(app: &mut AppState, opts: &SnapshotOptions) {
    if opts.attach_persistent {
        app.attach_detached_persistent_tabs();
        // O shell novo da aba reaberta desenha o prompt e o comando digitado.
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            app.process_output();
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    for (index, target, color) in &opts.tab_colors {
        app.set_tab_color(*index, *target, Some(*color));
    }
    for (index, title) in &opts.renames {
        app.rename_tab(*index, Some(title));
    }
    // Depois dos títulos: a aba persistente já vai ao daemon com o dela.
    for index in &opts.persist {
        if let Err(e) = app.make_tab_persistent(*index) {
            tracing::warn!(tab = index, error = %e, "Snapshot: could not make tab persistent");
        }
    }
    for (child, parent) in &opts.nests {
        app.nest_tab(*child, *parent);
    }
    for index in &opts.collapse {
        app.toggle_tab_group(*index);
    }
    if let Some(index) = opts.active {
        app.go_to_tab(index);
    }
    if let Some((index, y)) = opts.drag {
        let row = app
            .tab_manager
            .visible_tabs()
            .iter()
            .position(|&tab| tab == index)
            .unwrap_or(0);
        let start = crate::renderer::TAB_BAR_HEIGHT as f32
            + (row as f32 + 0.5) * crate::ui::sidebar_rows::SIDEBAR_ENTRY_HEIGHT;
        app.sidebar_press(index, 60.0, start);
        app.update_sidebar_drag(70.0, y);
    }
}

/// Executa o snapshot e grava o PNG.
pub fn run(opts: &SnapshotOptions) -> Result<SnapshotReport> {
    let mut config = Config::load().unwrap_or_default();
    if let Some(theme) = &opts.theme {
        if !Theme::apply_to_config(theme, &mut config) {
            bail!("unknown theme '{theme}'");
        }
    }
    if let Some(size) = opts.font_size {
        config.font.size = size;
    }

    crate::renderer::text::preload_fonts();
    let started = Instant::now();
    let mut gpu = pollster::block_on(GpuState::new_headless(
        opts.width,
        opts.height,
        &config.font,
        &config.colors,
    ))?;
    let gpu_time = started.elapsed();

    let (cell_width, cell_height) = gpu.cell_size();
    let sidebar = if config.sidebar.visible {
        config.sidebar.width.max(120.0)
    } else {
        0.0
    };
    let (cols, rows) = GridLayout::below_tab_bar(cell_width, cell_height)
        .with_left_inset(sidebar)
        .grid_size(opts.width, opts.height);
    let (program, args) = program_and_args(&opts.command);

    let output_started = Instant::now();
    let mut app = AppState::with_command(config, cols, rows, &program, &args)?;
    app.desktop_notifications = false;
    app.set_cell_pixel_size(cell_width, cell_height);
    if opts.overlay == Some(SnapshotOverlay::Inspector) {
        // Liga antes de rodar o comando, para gravar a saída dele.
        app.toggle_inspector();
    }
    if let Some(dir) = &opts.plugins {
        let state =
            std::env::temp_dir().join(format!("code-terminal-snapshot-{}", std::process::id()));
        let loaded = app.enable_plugins(dir, &state);
        tracing::info!(loaded, dir = %dir.display(), "Snapshot plugins");
    }
    for command in &opts.tabs {
        app.open_tab_running("sh", &["-c".to_string(), command.clone()])?;
    }
    for (direction, command) in &opts.splits {
        let args = vec!["-c".to_string(), command.clone()];
        app.split_active_pane_running(*direction, "sh", &args)?;
    }
    pump_output(&mut app, opts.wait);
    app.refresh_sidebar(true);
    // A memória das tabs é medida em segundo plano: espera a primeira medição
    // para o print já mostrar o número.
    let memory_until = Instant::now() + Duration::from_millis(800);
    while cfg!(target_os = "linux")
        && app.config.sidebar.show_memory
        && Instant::now() < memory_until
        && app
            .sidebar_info()
            .iter()
            .any(|info| info.memory_kb.is_none())
    {
        std::thread::sleep(Duration::from_millis(20));
        app.refresh_sidebar(true);
    }
    apply_sidebar_options(&mut app, opts);
    if opts.notice {
        if let Some(pane) = app.active_tab().map(|tab| tab.focused_pane()) {
            app.notify_pane(pane, "build", "done");
        }
    }
    if let Some((x, y)) = opts.hover {
        let layout =
            GridLayout::below_tab_bar(cell_width, cell_height).with_left_inset(app.sidebar_width());
        let hover = crate::ui::window::hover_at(&app, &layout, opts.height as f32, x, y);
        app.set_hover(hover);
    }
    open_overlay(&mut app, opts.overlay);
    if let Some(section) = opts.section {
        app.settings_click(crate::app::SettingsClick::Section(section));
    }
    for key in &opts.settings_keys {
        app.handle_settings_key(key.clone());
    }
    let output_time = output_started.elapsed();

    let render_started = Instant::now();
    let (under, over) = compose_frame(&app, &mut gpu, opts.width as f32, opts.height as f32, true);
    let image = gpu.capture(Some(&under), Some(&over))?;
    image
        .save(&opts.output)
        .with_context(|| format!("failed to write {:?}", opts.output))?;
    let per_frame = (opts.frames > 0).then(|| {
        // Como a janela faz a cada quadro da animação: monta e desenha.
        let (cpu0, wall0) = (process_cpu(), Instant::now());
        for _ in 0..opts.frames {
            let (under, over) =
                compose_frame(&app, &mut gpu, opts.width as f32, opts.height as f32, true);
            let _ = gpu.render_layers(Some(&under), Some(&over));
        }
        let cpu = process_cpu().saturating_sub(cpu0) / opts.frames;
        (cpu, wall0.elapsed() / opts.frames)
    });

    let settings_memory = (opts.settings_cycles > 0).then(|| {
        let cycle = |app: &mut AppState, gpu: &mut GpuState, i: u32| {
            app.show_settings();
            app.settings_click(crate::app::SettingsClick::Section(i as usize % 5));
            let _ = compose_frame(app, gpu, opts.width as f32, opts.height as f32, true);
            app.hide_settings();
            let _ = compose_frame(app, gpu, opts.width as f32, opts.height as f32, true);
        };
        let before = private_kb();
        for i in 0..10 {
            cycle(&mut app, &mut gpu, i);
        }
        let after_10 = private_kb();
        for i in 10..opts.settings_cycles.max(10) {
            cycle(&mut app, &mut gpu, i);
        }
        (before, after_10, private_kb())
    });

    Ok(SnapshotReport {
        gpu: gpu_time,
        output: output_time,
        render: render_started.elapsed(),
        cols,
        rows,
        settings_memory,
        per_frame,
    })
}

/// Memória privada do processo agora, em kB (Linux; 0 nos outros).
fn private_kb() -> u64 {
    std::fs::read_to_string("/proc/self/smaps_rollup")
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("Private_Clean:") || line.starts_with("Private_Dirty:"))
        .filter_map(|line| line.split_whitespace().nth(1)?.parse::<u64>().ok())
        .sum()
}

/// CPU usada pelo processo até agora (soma das threads; Linux).
fn process_cpu() -> Duration {
    let mut total_ns = 0u64;
    if let Ok(tasks) = std::fs::read_dir("/proc/self/task") {
        for task in tasks.flatten() {
            let stat = std::fs::read_to_string(task.path().join("schedstat")).unwrap_or_default();
            total_ns += stat
                .split_whitespace()
                .next()
                .and_then(|ns| ns.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }
    Duration::from_nanos(total_ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_settings_cycles() {
        let opts = parse_args(args(&["--snapshot", "o.png", "--settings-cycles", "500"]))
            .unwrap()
            .unwrap();
        assert_eq!(opts.settings_cycles, 500);
    }

    #[test]
    fn test_no_snapshot_flag_opens_window() {
        assert_eq!(parse_args(args(&[])).unwrap(), None);
    }

    #[test]
    fn test_parse_notice_and_frames() {
        let opts = parse_args(args(&["--snapshot", "o.png", "--notice", "--frames", "90"]))
            .unwrap()
            .unwrap();
        assert!(opts.notice);
        assert_eq!(opts.frames, 90);
        assert!(parse_args(args(&["--snapshot", "o.png", "--frames", "x"])).is_err());
    }

    #[test]
    fn test_parse_tab_colors() {
        use crate::ui::tab_colors::ColorTarget;
        let opts = parse_args(args(&[
            "--snapshot",
            "o.png",
            "--tab-color",
            "1=#ff8000",
            "--tab-bg",
            "0=102030",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            opts.tab_colors,
            vec![
                (1, ColorTarget::Tab, [255, 128, 0]),
                (0, ColorTarget::Background, [16, 32, 48])
            ]
        );
        assert!(parse_args(args(&["--snapshot", "o.png", "--tab-bg", "0=zz"])).is_err());
    }

    #[test]
    fn test_parse_hover() {
        let opts = parse_args(args(&["--snapshot", "o.png", "--hover", "120:64.5"]))
            .unwrap()
            .unwrap();
        assert_eq!(opts.hover, Some((120.0, 64.5)));
        assert!(parse_args(args(&["--snapshot", "o.png", "--hover", "12"])).is_err());
    }

    #[test]
    fn test_parse_settings_keys() {
        use crate::ui::settings::SettingsInputKey as K;
        let opts = parse_args(args(&[
            "--snapshot",
            "o.png",
            "--settings-keys",
            "down,enter,a,comma,bs",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            opts.settings_keys,
            vec![K::Down, K::Enter, K::Char('a'), K::Char(','), K::Backspace]
        );
        assert!(parse_args(args(&["--snapshot", "o.png", "--settings-keys", "nope"])).is_err());
    }

    #[test]
    fn test_parse_themes_overlay() {
        assert_eq!(
            SnapshotOverlay::parse("themes").unwrap(),
            SnapshotOverlay::Themes
        );
    }

    #[test]
    fn test_parse_full_snapshot_args() {
        let opts = parse_args(args(&[
            "--snapshot",
            "out.png",
            "--size",
            "640x480",
            "--wait-ms",
            "200",
            "--overlay",
            "palette",
            "--theme",
            "dracula",
            "--font-size",
            "14",
            "--split-h",
            "top",
            "--split-v",
            "htop -d 5",
            "--plugins",
            "/tmp/plugins",
            "--",
            "ls",
            "--color=always",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(opts.output, PathBuf::from("out.png"));
        assert_eq!((opts.width, opts.height), (640, 480));
        assert_eq!(opts.wait, Duration::from_millis(200));
        assert_eq!(opts.plugins, Some(PathBuf::from("/tmp/plugins")));
        assert_eq!(opts.overlay, Some(SnapshotOverlay::Palette));
        assert_eq!(opts.theme.as_deref(), Some("dracula"));
        assert_eq!(opts.font_size, Some(14.0));
        assert_eq!(
            opts.splits,
            vec![
                (SplitDirection::Horizontal, "top".to_string()),
                (SplitDirection::Vertical, "htop -d 5".to_string())
            ]
        );
        assert_eq!(opts.command, args(&["ls", "--color=always"]));
    }

    #[test]
    fn test_parse_sidebar_options() {
        let opts = parse_args(args(&[
            "--snapshot",
            "out.png",
            "--rename",
            "0=api = prod",
            "--nest",
            "2:0",
            "--collapse",
            "0",
            "--active",
            "1",
            "--drag",
            "3:120.5",
            "--overlay",
            "tab-menu",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(opts.renames, vec![(0, "api = prod".to_string())]);
        assert_eq!(opts.nests, vec![(2, 0)]);
        assert_eq!(opts.collapse, vec![0]);
        assert_eq!(opts.active, Some(1));
        assert_eq!(opts.drag, Some((3, 120.5)));
        assert_eq!(opts.overlay, Some(SnapshotOverlay::TabMenu));
        let rename = parse_args(args(&["--snapshot", "a.png", "--overlay", "rename"]));
        assert_eq!(
            rename.unwrap().unwrap().overlay,
            Some(SnapshotOverlay::Rename)
        );
        for bad in [
            &["--rename", "x"][..],
            &["--nest", "1"],
            &["--nest", "a:1"],
            &["--drag", "1:y"],
            &["--active", "-1"],
        ] {
            let mut list = vec!["--snapshot", "a.png"];
            list.extend_from_slice(bad);
            assert!(parse_args(args(&list)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn test_parse_errors() {
        assert!(parse_args(args(&["--size", "0x10", "--snapshot", "a.png"])).is_err());
        assert!(parse_args(args(&["--snapshot"])).is_err());
        assert!(parse_args(args(&["--snapshot", "a.png", "--bogus"])).is_err());
        assert_eq!(parse_args(args(&["--bogus"])).unwrap(), None);
        assert!(parse_args(args(&["--snapshot", "a.png", "--overlay", "x"])).is_err());
        assert_eq!(parse_args(args(&["--theme", "dark"])).unwrap(), None);
    }

    #[test]
    fn test_program_and_args() {
        let (program, rest) = program_and_args(&args(&["ls", "-l"]));
        assert_eq!(program, "ls");
        assert_eq!(rest, args(&["-l"]));
        let (_, rest) = program_and_args(&[]);
        assert_eq!(rest, args(&["-i"]));
    }
}
