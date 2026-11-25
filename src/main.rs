#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! Code Terminal - Terminal Emulator em Rust
//!
//! Entry point principal que inicializa o WindowManager
//! e delega o gerenciamento de eventos para a UI.

use anyhow::Result;
use code_terminal::app::snapshot;
use code_terminal::app::state::AppState;
use code_terminal::config::keybindings::{
    Action, ClipboardShortcuts, NativeSplitShortcuts, SearchShortcuts, TabShortcuts,
};
use code_terminal::config::Config;
use code_terminal::multiplexer::socket_api::{ApiServer, ApiServerConfig};
use code_terminal::terminal::modes::CursorKeysMode;
use code_terminal::ui::command_palette::{PaletteAction, PaletteInputKey};
use code_terminal::ui::hover::Hover;
use code_terminal::ui::key_encoding::{encode_key, KeyMods};
use code_terminal::ui::mouse_report::{self, MouseAction, MouseButton as ReportButton, MouseMods};
use code_terminal::ui::mouse_selection::{MouseButton, MouseSelectionEvent};
use code_terminal::ui::overlay_hits::{self, OverlayKind, OverlayMouse};
use code_terminal::ui::pane_layout::{FocusDirection, SplitDirection};
use code_terminal::ui::settings::SettingsInputKey;
use code_terminal::ui::window::{
    is_on_rename_field, sidebar_drop_target, sidebar_entry_at, Window, WindowCreateInfo,
    WindowEventMsg, WindowId, WindowManager,
};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::{
    event::{ElementState, Event, KeyEvent, Modifiers, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::WindowAttributes,
};

/// O que a janela deve fazer depois de uma tecla.
#[derive(Debug, PartialEq, Eq)]
enum KeyOutcome {
    Handled,
    CloseWindow,
}

/// Acorda o event loop quando um PTY escreve. Várias leituras seguidas viram
/// um único evento até o loop processá-lo, para não inundar a fila.
fn install_output_waker(proxy: EventLoopProxy<WindowEventMsg>) -> Arc<AtomicBool> {
    let pending = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&pending);
    let proxy = Mutex::new(proxy);
    code_terminal::terminal::pty::set_output_waker(move || {
        if !flag.swap(true, Ordering::AcqRel) {
            let _ = proxy.lock().send_event(WindowEventMsg::PtyOutput);
        }
    });
    pending
}

/// Liga a Socket API ao estado real: a chamada vai para o event loop (dono do
/// `AppState`) e a thread do servidor espera a resposta.
fn install_api_bridge(proxy: EventLoopProxy<WindowEventMsg>) {
    let proxy = Mutex::new(proxy);
    code_terminal::multiplexer::socket_api::set_app_bridge(Arc::new(move |method, params| {
        if !code_terminal::app::api::handles(method) {
            return None;
        }
        let (reply, response) = std::sync::mpsc::channel();
        let message = WindowEventMsg::Api {
            method: method.to_string(),
            params: params.clone(),
            reply,
        };
        if proxy.lock().send_event(message).is_err() {
            return Some(Err(
                code_terminal::multiplexer::socket_api::ApiError::InvalidState(
                    "window is closing".into(),
                ),
            ));
        }
        match response.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(result) => result,
            Err(_) => Some(Err(
                code_terminal::multiplexer::socket_api::ApiError::Timeout,
            )),
        }
    }));
}

/// Inicializa o GPU de uma janela e ajusta o terminal à área disponível.
fn init_window_gpu(window_manager: &mut WindowManager, window_id: WindowId) {
    let Some(window) = window_manager.get_window_mut(window_id) else {
        return;
    };
    let gpu_config = window
        .app_state
        .as_ref()
        .map(|app| (app.config.font.clone(), app.config.colors.clone()));
    let started = std::time::Instant::now();
    code_terminal::renderer::gpu::keep_shared_gpu();
    match pollster::block_on(window.initialize_gpu(gpu_config.as_ref().map(|(f, c)| (f, c)))) {
        Ok(()) => {
            window.fit_terminal_to_window();
            let size = window
                .app_state
                .as_ref()
                .map(|app| app.terminal_size)
                .unwrap_or_default();
            tracing::info!(
                cols = size.0,
                rows = size.1,
                gpu_ms = started.elapsed().as_millis() as u64,
                "Terminal initialized"
            );
            show_window(window);
        }
        Err(e) => {
            tracing::error!("Failed to initialize GPU: {}", e);
            // Sem GPU a janela não desenha, mas não pode ficar invisível.
            window.winit_window.set_visible(true);
        }
    }
}

/// Reabre as tabs, painéis e diretórios da última vez (primeira janela).
fn restore_workspace(window_manager: &mut WindowManager, window_id: WindowId) {
    use code_terminal::session::workspace::{default_workspace_path, load_workspace};
    let Some(app) = window_manager
        .get_window_mut(window_id)
        .and_then(|window| window.app_state.as_mut())
    else {
        return;
    };
    if !app.config.session.restore {
        return;
    }
    let Some(snapshot) = default_workspace_path().and_then(|path| load_workspace(&path)) else {
        return;
    };
    match app.restore_workspace(&snapshot) {
        Ok(()) => tracing::info!(tabs = snapshot.tabs.len(), "Workspace restored"),
        Err(e) => tracing::warn!(error = %e, "Failed to restore workspace"),
    }
}

/// As tabs persistentes que ficaram sem janela (fechou a janela, reiniciou
/// o servidor) voltam nesta, no mesmo lugar e nos mesmos grupos.
fn attach_persistent_tabs(window_manager: &mut WindowManager, window_id: WindowId) {
    let Some(app) = window_manager
        .get_window_mut(window_id)
        .and_then(|window| window.app_state.as_mut())
    else {
        return;
    };
    let count = app.attach_detached_persistent_tabs();
    if count > 0 {
        tracing::info!(count, "Persistent tabs attached");
    }
}

/// Carrega os plugins WASM de `<config>/plugins` na janela principal.
fn enable_plugins(window_manager: &mut WindowManager, window_id: WindowId) {
    let Some(app) = window_manager
        .get_window_mut(window_id)
        .and_then(|window| window.app_state.as_mut())
    else {
        return;
    };
    let Some(dir) = code_terminal::app::plugins::default_plugins_dir() else {
        return;
    };
    if !dir.is_dir() {
        return;
    }
    let data_root = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("code-terminal")
        .join("plugins");
    let count = app.enable_plugins(&dir, &data_root);
    tracing::info!(count, dir = %dir.display(), "Plugins enabled");
}

/// Salva o workspace da janela que vai fechar. Sem tabs (o usuário fechou
/// tudo), apaga o arquivo: a próxima abertura começa do zero.
fn save_workspace_of(window_manager: &WindowManager, window_id: WindowId) {
    use code_terminal::session::workspace::{
        clear_workspace, default_workspace_path, save_workspace,
    };
    let Some(path) = default_workspace_path() else {
        return;
    };
    let Some(app) = window_manager
        .get_window(window_id)
        .and_then(|window| window.app_state.as_ref())
    else {
        return;
    };
    if !app.config.session.restore {
        return;
    }
    let snapshot = app.capture_workspace();
    if snapshot.tabs.is_empty() {
        clear_workspace(&path);
    } else if let Err(e) = save_workspace(&path, &snapshot) {
        tracing::warn!(error = %e, "Failed to save workspace");
    }
}

/// Atributos de uma janela nova. Ela nasce invisível e só aparece com a GPU
/// pronta ([`show_window`]): antes disso o sistema pintaria o fundo padrão
/// (branco no macOS e no Windows). Maximizada se `window.start_maximized`
/// (no macOS, "zoom": preenche a tela, sem tela cheia em outro Space).
/// `CODE_TERMINAL_WINDOW_SIZE=LxA` força uma janela comum desse tamanho
/// (medições e prints).
fn window_attributes(
    title: &str,
    size: (u32, u32),
    window: &code_terminal::config::WindowConfig,
) -> WindowAttributes {
    let forced = forced_window_size();
    let size = forced.unwrap_or(size);
    WindowAttributes::default()
        .with_title(title)
        .with_inner_size(winit::dpi::LogicalSize::new(size.0, size.1))
        .with_maximized(window.start_maximized && forced.is_none())
        .with_visible(false)
}

/// Tamanho pedido em `CODE_TERMINAL_WINDOW_SIZE` (`1280x800`).
fn forced_window_size() -> Option<(u32, u32)> {
    let value = std::env::var("CODE_TERMINAL_WINDOW_SIZE").ok()?;
    let (w, h) = value.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Mostra a janela já desenhada com o tema. No macOS e no Windows o primeiro
/// frame sai com ela ainda invisível (nem um quadro de fundo branco). No
/// Linux ela aparece logo que a GPU fica pronta: no X11 apresentar numa
/// janela não mapeada pode travar, e no Wayland a janela só aparece com o
/// primeiro buffer de qualquer jeito.
fn show_window(window: &mut Window) {
    #[cfg(any(target_os = "macos", windows))]
    if let Err(e) = window.render() {
        tracing::warn!(error = %e, "First frame before showing the window failed");
    }
    window.winit_window.set_visible(true);
    window.winit_window.request_redraw();
}

/// Fecha a janela e encerra o programa se for a última. O servidor continua
/// de pé sem janelas e não mexe no workspace salvo (cada janela dele é um
/// terminal novo, como o `footclient`).
fn close_window(window_manager: &mut WindowManager, id: WindowId, elwt: &ActiveEventLoop) {
    // As tabs persistentes desanexam: o daemon guarda onde cada uma estava.
    if let Some(app) = window_manager
        .get_window_mut(id)
        .and_then(|window| window.app_state.as_mut())
    {
        app.sync_persistent_meta(true);
    }
    if !server_mode() {
        save_workspace_of(window_manager, id);
    }
    if !window_manager.close_window(id) {
        if server_mode() {
            schedule_gpu_release();
        } else {
            elwt.exit();
        }
    }
}

/// Prazo para soltar a GPU sem janela: `CODE_TERMINAL_RELEASE_GPU_AFTER`
/// (mesmo formato) ou `server.release_gpu_after` lido do disco agora, para
/// valer a escolha feita em Configurações sem reiniciar o servidor.
fn gpu_release_delay() -> Option<std::time::Duration> {
    use code_terminal::config::parse_release_after;
    if let Some(delay) = std::env::var("CODE_TERMINAL_RELEASE_GPU_AFTER")
        .ok()
        .and_then(|value| parse_release_after(&value))
    {
        return delay;
    }
    Config::load()
        .unwrap_or_default()
        .server
        .release_gpu_after()
}

/// Dorme o prazo (uma acordada só) e solta a GPU se nenhuma janela abriu
/// desde `ticket`. "Nunca" não agenda nada.
fn release_gpu_when_idle(ticket: u64) {
    if let Some(delay) = gpu_release_delay() {
        code_terminal::renderer::gpu::release_shared_gpu_if_idle(ticket, delay);
    }
}

/// Servidor sem janelas: solta a GPU depois do prazo, se nenhuma janela
/// abrir antes (a thread dorme o prazo inteiro e acorda uma vez só).
fn schedule_gpu_release() {
    use code_terminal::renderer::gpu;
    let ticket = gpu::gpu_idle_ticket();
    let spawned = std::thread::Builder::new()
        .name("gpu-release".into())
        .spawn(move || {
            release_gpu_when_idle(ticket);
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "Could not schedule the GPU release");
    }
}

/// Processo rodando como `code-terminal --server`: fica de pé sem janelas.
static SERVER_MODE: AtomicBool = AtomicBool::new(false);

fn server_mode() -> bool {
    SERVER_MODE.load(Ordering::Relaxed)
}

/// Janelas do modo servidor esperando o primeiro frame para responder.
type PendingOpens = std::collections::HashMap<WindowId, PendingOpen>;

struct PendingOpen {
    reply: std::sync::mpsc::Sender<Result<u64, String>>,
    requested: std::time::Instant,
    close_after_first_frame: bool,
}

/// Com um servidor de pé, pede a janela a ele e devolve `true` (este processo
/// pode sair). Sem servidor, ou se ele falhar, devolve `false` e a janela
/// abre aqui mesmo.
#[cfg(unix)]
fn open_via_server(startup: std::time::Instant, measure: bool) -> bool {
    use code_terminal::app::server;
    if server::server_disabled_by_env() {
        return false;
    }
    let request = server::OpenRequest {
        cwd: std::env::current_dir().ok(),
        close_after_first_frame: measure,
    };
    match server::request_window(&server::default_server_socket(), &request) {
        None => false,
        Some(Ok(reply)) if reply.ok => {
            if measure {
                println!("first_frame_ms={}", startup.elapsed().as_millis());
            }
            true
        }
        Some(Ok(reply)) => {
            eprintln!(
                "code-terminal server could not open a window ({}); opening here",
                reply.error.unwrap_or_default()
            );
            false
        }
        Some(Err(e)) => {
            eprintln!("code-terminal server did not answer ({e}); opening here");
            false
        }
    }
}

/// Liga o modo servidor: GPU ligada já (em outra thread) e o socket que
/// recebe os pedidos de janela.
#[cfg(unix)]
fn start_server(
    proxy: EventLoopProxy<WindowEventMsg>,
) -> Result<code_terminal::app::server::ServerGuard> {
    use code_terminal::app::server;
    use code_terminal::renderer::gpu;
    let ticket = gpu::gpu_idle_ticket();
    let prewarm = std::thread::Builder::new()
        .name("gpu-prewarm".into())
        .spawn(move || {
            if let Err(e) = pollster::block_on(gpu::GpuState::prewarm()) {
                tracing::warn!(error = %e, "Could not start the GPU ahead of the first window");
            }
            // Ninguém pediu janela no prazo: solta a GPU (o driver e a
            // thread dele saem; a primeira janela liga de novo).
            release_gpu_when_idle(ticket);
        });
    if let Err(e) = prewarm {
        tracing::warn!(error = %e, "Could not start the GPU ahead of the first window");
    }
    let proxy = Mutex::new(proxy);
    let path = server::default_server_socket();
    let guard = server::start(&path, move |request, reply| {
        let message = WindowEventMsg::ServerOpen {
            cwd: request.cwd,
            close_after_first_frame: request.close_after_first_frame,
            reply: reply.clone(),
        };
        if proxy.lock().send_event(message).is_err() {
            let _ = reply.send(Err("server is shutting down".into()));
        }
    })?;
    tracing::info!(socket = %guard.path().display(), "Server mode: waiting for window requests");
    Ok(guard)
}

#[cfg(not(unix))]
fn start_server(_proxy: EventLoopProxy<WindowEventMsg>) -> Result<()> {
    anyhow::bail!("--server is only available on Linux and macOS")
}

/// Abre a janela pedida ao servidor; a resposta sai no primeiro frame.
fn open_server_window(
    window_manager: &mut WindowManager,
    elwt: &ActiveEventLoop,
    config: &Config,
    request: PendingRequest,
    pending: &mut PendingOpens,
) {
    let requested = std::time::Instant::now();
    let size = (config.window.width, config.window.height);
    let create_info = WindowCreateInfo {
        title: config.window.title.clone(),
        size,
        position: None,
        config: config.window.clone(),
        cwd: request.cwd,
    };
    let result = window_manager.create_window(create_info, || {
        let window = elwt
            .create_window(window_attributes("Code Terminal", size, &config.window))
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(Arc::new(window))
    });
    match result {
        Ok(id) => {
            window_manager.set_active_window(id);
            init_window_gpu(window_manager, id);
            attach_persistent_tabs(window_manager, id);
            enable_plugins(window_manager, id);
            pending.insert(
                id,
                PendingOpen {
                    reply: request.reply,
                    requested,
                    close_after_first_frame: request.close_after_first_frame,
                },
            );
        }
        Err(e) => {
            tracing::error!("Failed to open window for the server: {}", e);
            let _ = request.reply.send(Err(e.to_string()));
        }
    }
}

/// Pedido de janela vindo do socket do servidor.
struct PendingRequest {
    cwd: Option<std::path::PathBuf>,
    close_after_first_frame: bool,
    reply: std::sync::mpsc::Sender<Result<u64, String>>,
}

/// Responde ao cliente do servidor quando a janela desenha pela primeira vez.
/// Devolve `true` quando a janela deve fechar (pedido de medição).
fn answer_pending_open(pending: &mut PendingOpens, id: WindowId) -> bool {
    let Some(open) = pending.remove(&id) else {
        return false;
    };
    let ms = open.requested.elapsed().as_millis() as u64;
    tracing::info!(first_frame_ms = ms, "Server window presented");
    let _ = open.reply.send(Ok(ms));
    open.close_after_first_frame
}

/// Com a feature `dhat-heap`, todo o heap do Rust passa pelo DHAT (#1).
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Liga o perfil de heap e grava o retrato depois de
/// `CODE_TERMINAL_DHAT_SECS` (padrão 20 s): o "t-end" do arquivo é o heap
/// vivo naquele instante, com a janela já parada.
#[cfg(feature = "dhat-heap")]
fn start_heap_profile() {
    let file = std::env::var("CODE_TERMINAL_DHAT_FILE").unwrap_or_else(|_| "dhat-heap.json".into());
    let secs = std::env::var("CODE_TERMINAL_DHAT_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    let profiler = dhat::Profiler::builder().file_name(file).build();
    let _ = code_terminal::utils::spawn_named("dhat", move || {
        std::thread::sleep(std::time::Duration::from_secs(secs));
        drop(profiler);
        tracing::info!("Heap profile written");
    });
}

#[allow(deprecated)]
fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    start_heap_profile();
    let startup = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wants_server = args.iter().any(|arg| arg == "--server");
    // Para medir a abertura: sai logo depois do primeiro frame.
    let exit_after_first_frame = std::env::var_os("CODE_TERMINAL_EXIT_AFTER_FIRST_FRAME").is_some();
    // Servidor de pé: só pede a janela e sai (antes de carregar qualquer coisa).
    #[cfg(unix)]
    if !wants_server
        && !args.iter().any(|arg| arg == "--snapshot")
        && open_via_server(startup, exit_after_first_frame)
    {
        return Ok(());
    }
    SERVER_MODE.store(wants_server, Ordering::Relaxed);

    code_terminal::utils::logging::init_logging();
    // As fontes carregam em paralelo com a janela e a GPU.
    code_terminal::renderer::text::preload_fonts();

    match snapshot::parse_args(args) {
        Ok(Some(opts)) => {
            let report = snapshot::run(&opts)?;
            println!(
                "{} ({}x{} cells) gpu={}ms output={}ms render={}ms",
                opts.output.display(),
                report.cols,
                report.rows,
                report.gpu.as_millis(),
                report.output.as_millis(),
                report.render.as_millis()
            );
            if let Some((before, after_10, after_all)) = report.settings_memory {
                println!(
                    "settings_cycles private_kb: before={before} after_10={after_10} after_all={after_all}"
                );
            }
            if let Some((cpu, wall)) = report.per_frame {
                println!(
                    "per_frame cpu={:.3}ms wall={:.3}ms",
                    cpu.as_secs_f64() * 1000.0,
                    wall.as_secs_f64() * 1000.0
                );
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
    }

    tracing::info!("Starting code-terminal");

    // Carrega configuração
    let config = Config::load().unwrap_or_default();
    code_terminal::renderer::gpu::set_frame_latency(config.window.frame_latency);
    let shared_config = Arc::new(RwLock::new(config.clone()));
    let api_server = ApiServer::new(ApiServerConfig::default(), Arc::clone(&shared_config));
    if let Err(e) = api_server.start() {
        tracing::error!("Failed to start Socket API server: {}", e);
    }

    // Cria o WindowManager
    let mut window_manager = WindowManager::new(config.clone());

    // Event loop dorme até haver evento: nada de redesenhar sem motivo.
    let event_loop = EventLoop::<WindowEventMsg>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let event_proxy = event_loop.create_proxy();
    window_manager.set_event_proxy(event_proxy.clone());
    let output_pending = install_output_waker(event_proxy.clone());
    install_api_bridge(event_proxy.clone());
    let _server_guard = if wants_server {
        Some(start_server(event_proxy.clone())?)
    } else {
        None
    };
    // SIGTERM/SIGINT/SIGHUP: apaga o socket e sai (os shells recebem SIGHUP
    // quando o processo some).
    #[cfg(unix)]
    if let Some(guard) = _server_guard.as_ref() {
        let socket = guard.path().to_path_buf();
        code_terminal::app::server::on_exit_signal(move || {
            let _ = std::fs::remove_file(&socket);
            tracing::info!("Server stopped by signal");
            std::process::exit(0);
        });
    }
    let mut pending_opens = PendingOpens::new();

    let mut modifiers = Modifiers::default();
    let mut mouse_pressed = false;
    // Aperto que um painel aberto consumiu: a soltura dele não vai ao terminal.
    let mut overlay_press = overlay_hits::OverlayPress::default();
    let mut last_click_time = std::time::Instant::now();
    let mut click_count = 0u32;
    let mut cursor_position = (0.0, 0.0);
    // Botão segurado e última célula relatada ao programa (modos de mouse).
    // O servidor não tem "primeira janela" do processo: cada janela responde
    // ao seu cliente.
    let mut first_frame_logged = wants_server;
    // Medição de quadros (CODE_TERMINAL_FRAME_BENCH): só na primeira janela.
    let mut frame_bench = code_terminal::app::frame_bench::FrameBench::from_env(
        code_terminal::renderer::gpu::frame_latency(),
    );
    let mut reported_button: Option<ReportButton> = None;
    let mut last_reported_cell: Option<(usize, usize)> = None;

    let _ = event_loop.run(move |event, elwt| match event {
        Event::UserEvent(msg) => match msg {
            WindowEventMsg::Api {
                method,
                params,
                reply,
            } => {
                // A Socket API mexe na janela ativa (ou na primeira).
                let target = window_manager
                    .active_window()
                    .map(|window| window.id)
                    .or_else(|| window_manager.windows().next().map(|(id, _)| *id));
                let result = target
                    .and_then(|id| window_manager.get_window_mut(id))
                    .and_then(|window| {
                        let app = window.app_state.as_mut()?;
                        let result =
                            code_terminal::app::api::handle_api_request(app, &method, &params);
                        window.winit_window.request_redraw();
                        result
                    });
                let _ = reply.send(result);
            }
            WindowEventMsg::PtyOutput => {
                output_pending.store(false, Ordering::Release);
                for (_, window) in window_manager.windows_mut() {
                    window.winit_window.request_redraw();
                }
            }
            WindowEventMsg::CreateWindow { position, size } => {
                tracing::info!("Creating new window via event");
                let create_info = WindowCreateInfo {
                    title: "Code Terminal".to_string(),
                    size,
                    position,
                    config: config.window.clone(),
                    cwd: None,
                };
                let result = window_manager.create_window(create_info, || {
                    let window = elwt
                        .create_window(window_attributes("Code Terminal", size, &config.window))
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok(Arc::new(window))
                });
                match result {
                    Ok(id) => init_window_gpu(&mut window_manager, id),
                    Err(e) => tracing::error!("Failed to create window: {}", e),
                }
            }
            WindowEventMsg::ServerOpen {
                cwd,
                close_after_first_frame,
                reply,
            } => {
                let request = PendingRequest {
                    cwd,
                    close_after_first_frame,
                    reply,
                };
                open_server_window(
                    &mut window_manager,
                    elwt,
                    &config,
                    request,
                    &mut pending_opens,
                );
            }
            WindowEventMsg::CloseWindow(window_id) => {
                close_window(&mut window_manager, window_id, elwt);
            }
            WindowEventMsg::FocusWindow(window_id) => {
                window_manager.set_active_window(window_id);
            }
            WindowEventMsg::MoveWindow(window_id, x, y) => {
                if let Some(window) = window_manager.get_window(window_id) {
                    window
                        .winit_window
                        .set_outer_position(winit::dpi::Position::Physical(
                            winit::dpi::PhysicalPosition::new(x, y),
                        ));
                }
            }
            WindowEventMsg::ResizeWindow(window_id, width, height) => {
                if let Some(window) = window_manager.get_window(window_id) {
                    let applied =
                        window
                            .winit_window
                            .request_inner_size(winit::dpi::Size::Physical(
                                winit::dpi::PhysicalSize::new(width, height),
                            ));
                    if applied.is_none() {
                        tracing::debug!(
                            window_id = ?window_id,
                            width,
                            height,
                            "Window resize request was queued or unsupported by the platform"
                        );
                    }
                }
            }
            WindowEventMsg::Broadcast(msg) => {
                tracing::debug!("Broadcast message: {}", msg);
            }
        },
        Event::Resumed => {
            // Cria a janela primária apenas se não houver janelas (o servidor
            // começa sem janela e espera os pedidos).
            if !server_mode() && !window_manager.has_windows() {
                let size = (config.window.width, config.window.height);
                let create_info = WindowCreateInfo {
                    title: config.window.title.clone(),
                    size,
                    position: None,
                    config: config.window.clone(),
                    cwd: None,
                };

                match window_manager.create_primary_window(create_info, || {
                    let window = elwt
                        .create_window(window_attributes("Code Terminal", size, &config.window))
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    Ok(Arc::new(window))
                }) {
                    Ok(window_id) => {
                        init_window_gpu(&mut window_manager, window_id);
                        restore_workspace(&mut window_manager, window_id);
                        attach_persistent_tabs(&mut window_manager, window_id);
                        enable_plugins(&mut window_manager, window_id);
                    }
                    Err(e) => {
                        tracing::error!("Failed to create primary window: {}", e);
                        elwt.exit();
                    }
                }
            }
        }
        Event::WindowEvent {
            ref event,
            window_id: winit_window_id,
        } => {
            // Cada evento vai para a janela onde aconteceu (antes ia sempre
            // para a janela ativa, mesmo vindo de outra).
            let Some(id) = window_manager.handle_window_event(winit_window_id, event) else {
                return;
            };

            if matches!(event, WindowEvent::CloseRequested) {
                close_window(&mut window_manager, id, elwt);
                return;
            }

            let Some(window) = window_manager.get_window_mut(id) else {
                return;
            };
            let layout = window.grid_layout();
            let mut redraw = true;
            let mut should_close = false;

            match event {
                WindowEvent::Resized(physical_size) => {
                    if let Some(ref mut gpu) = window.gpu_state {
                        gpu.resize(*physical_size);
                    }
                    window.fit_terminal_to_window();
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    let size = window.winit_window.inner_size();
                    let font = window.app_state.as_ref().map(|app| app.config.font.clone());
                    if let (Some(gpu), Some(font)) = (window.gpu_state.as_mut(), font) {
                        gpu.set_scale_factor(*scale_factor, &font);
                        gpu.resize(size);
                    }
                    window.fit_terminal_to_window();
                }
                WindowEvent::Focused(focused) => {
                    if !focused {
                        mouse_pressed = false;
                        reported_button = None;
                    } else {
                        // A opção "reduzir movimento" pode ter mudado no
                        // sistema enquanto a janela estava sem foco.
                        code_terminal::ui::tab_animation::refresh_reduced_motion();
                    }
                    if let Some(app) = window.app_state.as_mut() {
                        app.set_window_focused(*focused);
                        let report = app
                            .active_tab()
                            .and_then(|tab| tab.grid())
                            .and_then(|grid| mouse_report::focus_report(grid.modes(), *focused));
                        if let Some(bytes) = report {
                            write_to_active_terminal(app, bytes, "focus report");
                        }
                    }
                }
                WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                    if let Some(ref mut app) = window.app_state {
                        write_to_active_terminal(app, text.as_bytes(), "IME commit");
                    }
                }
                WindowEvent::DroppedFile(path) => {
                    tracing::info!(path = %path.display(), "File dropped onto terminal window");
                    if let Some(ref mut app) = window.app_state {
                        let text = path.display().to_string();
                        write_to_active_terminal(app, text.as_bytes(), "dropped file path");
                    }
                }
                WindowEvent::ModifiersChanged(new_modifiers) => {
                    modifiers = *new_modifiers;
                    redraw = false;
                }
                WindowEvent::KeyboardInput {
                    event: key_event, ..
                } => {
                    if key_event.state == ElementState::Pressed {
                        if let Some(ref mut app) = window.app_state {
                            should_close =
                                handle_key_input(key_event, app, modifiers, &event_proxy)
                                    == KeyOutcome::CloseWindow;
                        }
                    } else {
                        redraw = false;
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => 'wheel: {
                    // Painel aberto: a roda rola o painel, nunca o terminal.
                    let (x, y) = (cursor_position.0 as f32, cursor_position.1 as f32);
                    let lines = wheel_lines(delta);
                    if let Some(outcome) = overlay_mouse_event(
                        window,
                        |hits| overlay_hits::route_wheel(hits, x, y, lines),
                        &event_proxy,
                    ) {
                        should_close = outcome == KeyOutcome::CloseWindow;
                        break 'wheel;
                    }
                    if let (Some(layout), Some(app)) = (layout, window.app_state.as_mut()) {
                        let (cols, rows) = app.terminal_size;
                        let (r, c) = layout.cell_at(
                            cursor_position.0,
                            cursor_position.1,
                            cols as usize,
                            rows as usize,
                        );
                        let cell = app.focused_pane_cell(r, c);
                        handle_mouse_wheel(delta, app, cell, &modifiers);
                    }
                }
                WindowEvent::CursorLeft { .. } => {
                    // Mouse saiu da janela: some com os botões de passar o mouse.
                    redraw = window
                        .app_state
                        .as_mut()
                        .is_some_and(|app| app.set_hover(Hover::None));
                }
                WindowEvent::CursorMoved { position, .. } => 'moved: {
                    cursor_position = (position.x, position.y);
                    redraw = mouse_pressed;

                    // Painel aberto: o mouse destaca o item sob o cursor e o
                    // movimento não vai ao programa do terminal.
                    let (x, y) = (position.x as f32, position.y as f32);
                    if overlay_mouse_event(
                        window,
                        |hits| overlay_hits::route_motion(hits, x, y),
                        &event_proxy,
                    )
                    .is_some()
                    {
                        if let Some(app) = window.app_state.as_mut() {
                            app.set_hover(Hover::None);
                        }
                        redraw = true;
                        break 'moved;
                    }

                    // Arrastando uma tab na sidebar: só redesenha a marca.
                    if let Some(app) = window.app_state.as_mut() {
                        if app.sidebar_drag().is_some() {
                            redraw = app.update_sidebar_drag(x, y);
                            break 'moved;
                        }
                    }

                    // Botões que aparecem com o mouse em cima (× e ⋯ do cartão,
                    // × do painel): redesenha só quando o alvo muda.
                    if let (Some(layout), Some(app)) = (layout, window.app_state.as_mut()) {
                        let height = window.winit_window.inner_size().height as f32;
                        let hover = code_terminal::ui::window::hover_at(app, &layout, height, x, y);
                        if app.set_hover(hover) {
                            redraw = true;
                        }
                    }

                    // Programa com 1002/1003 recebe o movimento (uma vez por célula).
                    if let (Some(layout), Some(app)) = (layout, window.app_state.as_mut()) {
                        let (cols, rows) = app.terminal_size;
                        let (r, c) = layout.cell_at(
                            cursor_position.0,
                            cursor_position.1,
                            cols as usize,
                            rows as usize,
                        );
                        let cell = app.focused_pane_cell(r, c);
                        if last_reported_cell != Some(cell) {
                            let action = MouseAction::Motion(reported_button);
                            if report_mouse(app, action, cell, &modifiers) {
                                last_reported_cell = Some(cell);
                                return;
                            }
                        }
                    }

                    if let (true, Some(layout), Some(app)) =
                        (mouse_pressed, layout, window.app_state.as_mut())
                    {
                        let (cols, rows) = app.terminal_size;
                        let (row, col) = layout.cell_at(
                            cursor_position.0,
                            cursor_position.1,
                            cols as usize,
                            rows as usize,
                        );
                        let (row, col) = app.focused_pane_cell(row, col);
                        app.handle_mouse_selection_event(MouseSelectionEvent::Drag {
                            row,
                            col,
                            alt: modifiers.state().alt_key(),
                        });
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => 'click: {
                    // Painel aberto: clique num item ativa, fora fecha; nada
                    // chega ao terminal (nem seleção, nem o programa do PTY).
                    let (x, y) = (cursor_position.0 as f32, cursor_position.1 as f32);
                    let pressed = *state == ElementState::Pressed;
                    let left = *button == winit::event::MouseButton::Left;
                    if !pressed && overlay_press.take_release() {
                        break 'click;
                    }
                    if let Some(app) = window.app_state.as_mut() {
                        // Tab arrastada na sidebar: soltar aplica (aninha ou move).
                        if !pressed && app.sidebar_drag().is_some() {
                            let target = sidebar_drop_target(app, x, y);
                            app.finish_sidebar_drag(target);
                            break 'click;
                        }
                        // Título em edição: clicar no campo não faz nada; clicar
                        // fora salva o que foi digitado e o clique segue.
                        if pressed && app.is_renaming_tab() {
                            if is_on_rename_field(app, x, y) {
                                break 'click;
                            }
                            app.commit_tab_rename();
                        }
                    }
                    // Configurações: seções e botões Salvar/Cancelar ficam fora
                    // da lista de opções.
                    if pressed && left {
                        let size = window.winit_window.inner_size();
                        let top_is_settings = window
                            .overlay_hits
                            .top()
                            .is_some_and(|hit| hit.kind == OverlayKind::Settings);
                        if let (true, Some(app)) = (top_is_settings, window.app_state.as_mut()) {
                            let click = app.settings_screen().and_then(|screen| {
                                code_terminal::ui::window::settings_click_at(
                                    screen,
                                    size.width as f32,
                                    size.height as f32,
                                    x,
                                    y,
                                )
                            });
                            if let Some(click) = click {
                                app.settings_click(click);
                                overlay_press.pressed(true);
                                break 'click;
                            }
                        }
                    }
                    let outcome = overlay_mouse_event(
                        window,
                        |hits| {
                            if !pressed {
                                return overlay_hits::route_release(hits);
                            }
                            match overlay_hits::route_click(hits, x, y) {
                                OverlayMouse::Activate(..) if !left => OverlayMouse::Consume,
                                route => route,
                            }
                        },
                        &event_proxy,
                    );
                    if pressed {
                        overlay_press.pressed(outcome.is_some());
                    }
                    if let Some(outcome) = outcome {
                        if !pressed {
                            // Soltura com painel aberto: um arraste do terminal
                            // termina aqui, sem seguir selecionando depois.
                            mouse_pressed = false;
                        }
                        should_close = outcome == KeyOutcome::CloseWindow;
                        break 'click;
                    }

                    // Botão direito num cartão da sidebar: menu da tab. Outros
                    // botões na sidebar não vão para o terminal.
                    if let Some(app) = window.app_state.as_mut() {
                        if !left && app.is_sidebar_visible() && x < app.sidebar_width() {
                            let right = *button == winit::event::MouseButton::Right;
                            let middle = *button == winit::event::MouseButton::Middle;
                            if let (true, Some(index)) = (pressed, sidebar_entry_at(app, x, y)) {
                                if right {
                                    app.open_tab_menu(index, x, y);
                                } else if middle && app.close_tab_from_mouse(index) {
                                    // Clique do meio fecha a tab, como nos navegadores.
                                    should_close = true;
                                    break 'click;
                                }
                            }
                            if y >= code_terminal::renderer::TAB_BAR_HEIGHT as f32 {
                                break 'click;
                            }
                        }
                    }

                    // Depois verifica se o clique foi na UI (tabs, botões).
                    if *button == winit::event::MouseButton::Left
                        && *state == ElementState::Pressed
                        && window.handle_mouse_click(cursor_position.0, cursor_position.1)
                    {
                        // O × do cartão ou do painel pode ter fechado a última tab.
                        if window
                            .app_state
                            .as_ref()
                            .is_some_and(|app| app.tab_count() == 0)
                        {
                            should_close = true;
                            break 'click;
                        }
                        window.winit_window.request_redraw();
                        return;
                    }

                    if let (Some(layout), Some(app)) = (layout, window.app_state.as_mut()) {
                        let (cols, rows) = app.terminal_size;
                        let (row, col) = layout.cell_at(
                            cursor_position.0,
                            cursor_position.1,
                            cols as usize,
                            rows as usize,
                        );
                        // Clique num painel dá foco a ele; a posição passa a ser
                        // relativa ao painel. Clique no separador é ignorado.
                        let (row, col) = if *state == ElementState::Pressed {
                            match app.pane_at(row, col) {
                                Some((pane, r, c)) => {
                                    app.focus_pane(pane);
                                    (r, c)
                                }
                                None => return,
                            }
                        } else {
                            app.focused_pane_cell(row, col)
                        };
                        // Ctrl+clique num link abre no navegador (antes do modo de
                        // mouse: vale também dentro do htop, nvim, tmux).
                        if *state == ElementState::Pressed
                            && *button == winit::event::MouseButton::Left
                            && modifiers.state().control_key()
                        {
                            if let Some(url) = app.link_at(row, col) {
                                app.open_link(&url);
                                return;
                            }
                        }
                        // Programa com modo de mouse (htop, nvim, tmux) recebe o clique.
                        if let Some(button) = report_button(*button) {
                            let action = match state {
                                ElementState::Pressed => MouseAction::Press(button),
                                ElementState::Released => MouseAction::Release(button),
                            };
                            if report_mouse(app, action, (row, col), &modifiers) {
                                reported_button =
                                    (*state == ElementState::Pressed).then_some(button);
                                last_reported_cell = Some((row, col));
                                return;
                            }
                        }
                        handle_mouse_button(
                            app,
                            *state,
                            *button,
                            row,
                            col,
                            modifiers.state().alt_key(),
                            &mut mouse_pressed,
                            &mut last_click_time,
                            &mut click_count,
                        );
                    }
                }
                WindowEvent::RedrawRequested => {
                    redraw = false;
                    if let Some(ref mut app) = window.app_state {
                        // Todas as tabs: as de fundo também enchem o canal do PTY.
                        app.process_output();
                        should_close = app.close_exited_tabs();
                        if let Some(title) = app.take_plugin_title() {
                            window.winit_window.set_title(&title);
                        }
                    }
                    if !should_close {
                        if let Err(e) = window.render() {
                            if e.to_string().contains("OutOfMemory") {
                                tracing::error!("GPU OutOfMemory, exiting");
                                elwt.exit();
                            }
                        }
                        should_close = answer_pending_open(&mut pending_opens, id);
                        if let (Some(bench), Some(app)) =
                            (frame_bench.as_mut(), window.app_state.as_mut())
                        {
                            match bench.on_frame(app, std::time::Instant::now()) {
                                code_terminal::app::frame_bench::BenchStep::Continue => {
                                    redraw = true;
                                }
                                code_terminal::app::frame_bench::BenchStep::Finished(report) => {
                                    tracing::info!(%report, "Frame bench finished");
                                    elwt.exit();
                                }
                            }
                        }
                        if !first_frame_logged {
                            first_frame_logged = true;
                            let ms = startup.elapsed().as_millis() as u64;
                            tracing::info!(startup_ms = ms, "First frame presented");
                            if exit_after_first_frame {
                                println!("first_frame_ms={ms}");
                                elwt.exit();
                            }
                        }
                    }
                }
                _ => redraw = false,
            }

            if should_close {
                close_window(&mut window_manager, id, elwt);
            } else if redraw {
                window.winit_window.request_redraw();
            }
        }
        _ => {}
    });

    code_terminal::terminal::pty::clear_output_waker();
    code_terminal::multiplexer::socket_api::clear_app_bridge();
    tracing::info!("Code Terminal closed");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_mouse_button(
    app: &mut AppState,
    state: ElementState,
    button: winit::event::MouseButton,
    row: usize,
    col: usize,
    alt: bool,
    mouse_pressed: &mut bool,
    last_click_time: &mut std::time::Instant,
    click_count: &mut u32,
) {
    let mouse_button = match button {
        winit::event::MouseButton::Left => MouseButton::Left,
        winit::event::MouseButton::Right => MouseButton::Right,
        winit::event::MouseButton::Middle => MouseButton::Middle,
        _ => MouseButton::Left,
    };

    match state {
        // Clique do meio cola a seleção PRIMARY (Linux), sem mexer na seleção.
        ElementState::Pressed if mouse_button == MouseButton::Middle => {
            app.paste_and_notify(true);
        }
        ElementState::Pressed => {
            *mouse_pressed = true;
            let now = std::time::Instant::now();

            // Detecta double/triple click
            if now.duration_since(*last_click_time).as_millis() < 500 {
                *click_count += 1;
            } else {
                *click_count = 1;
            }
            *last_click_time = now;

            if let Some(grid) = app.active_tab_mut().and_then(|tab| tab.grid_mut()) {
                if *click_count >= 3 {
                    grid.select_line_at(row);
                } else if *click_count == 2 {
                    grid.select_word_at(row, col);
                } else {
                    use code_terminal::terminal::grid::SelectionKind;
                    grid.start_selection(row, col, SelectionKind::Normal);
                }
            }
            if *click_count >= 3 {
                *click_count = 0;
            }

            app.handle_mouse_selection_event(MouseSelectionEvent::Press {
                row,
                col,
                button: mouse_button,
                alt,
            });
        }
        ElementState::Released => {
            *mouse_pressed = false;
            app.handle_mouse_selection_event(MouseSelectionEvent::Release {
                row,
                col,
                button: mouse_button,
            });
            // Copiar ao selecionar (arraste, duplo ou triplo clique).
            if mouse_button == MouseButton::Left {
                app.copy_on_select();
            }
        }
    }
}

enum ClipboardPlatform {
    Macos,
    Windows,
    Other,
}

impl ClipboardPlatform {
    const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Macos
    } else if cfg!(windows) {
        Self::Windows
    } else {
        Self::Other
    };

    fn shortcut(
        &self,
        modifiers: ModifiersState,
        key: &str,
        grid: Option<&code_terminal::terminal::grid::Grid>,
    ) -> Option<Action> {
        use code_terminal::app::state::selection_worth_copying;
        match (self, modifiers, key) {
            (Self::Macos, ModifiersState::SUPER, "c") => Some(Action::Copy),
            (Self::Macos, ModifiersState::SUPER, "v") => Some(Action::Paste),
            (Self::Windows, ModifiersState::CONTROL, "c")
                if grid.is_some_and(|grid| selection_worth_copying(grid.selection())) =>
            {
                Some(Action::Copy)
            }
            // Ctrl+V é usado por editores e outros programas na tela alternativa.
            (Self::Windows, ModifiersState::CONTROL, "v")
                if grid.is_some_and(|grid| !grid.using_alternate_buffer()) =>
            {
                Some(Action::Paste)
            }
            _ => None,
        }
    }
}

fn handle_key_input(
    key_event: &KeyEvent,
    app: &mut AppState,
    modifiers: Modifiers,
    event_proxy: &EventLoopProxy<WindowEventMsg>,
) -> KeyOutcome {
    let mods = modifiers.state();
    let (ctrl, shift, alt) = (mods.control_key(), mods.shift_key(), mods.alt_key());
    let tab_shortcuts = TabShortcuts::from_config(&app.config.keybindings);
    let clipboard_shortcuts = ClipboardShortcuts::from_config(&app.config.keybindings);
    let search_shortcuts = SearchShortcuts::from_config(&app.config.keybindings);
    let key_str = physical_key_to_string(&key_event.physical_key);
    let platform_clipboard =
        ClipboardPlatform::CURRENT.shortcut(mods, &key_str, app.active_tab_grid());

    // Título de tab em edição na sidebar: as teclas vão para o campo.
    if app.is_renaming_tab() {
        use code_terminal::ui::line_edit::LineEditKey;
        let key = match &key_event.logical_key {
            Key::Named(NamedKey::Enter) => {
                app.commit_tab_rename();
                return KeyOutcome::Handled;
            }
            Key::Named(NamedKey::Escape) => {
                app.cancel_tab_rename();
                return KeyOutcome::Handled;
            }
            Key::Named(NamedKey::Backspace) => Some(LineEditKey::Backspace),
            Key::Named(NamedKey::Delete) => Some(LineEditKey::Delete),
            Key::Named(NamedKey::ArrowLeft) => Some(LineEditKey::Left),
            Key::Named(NamedKey::ArrowRight) => Some(LineEditKey::Right),
            Key::Named(NamedKey::Home) => Some(LineEditKey::Home),
            Key::Named(NamedKey::End) => Some(LineEditKey::End),
            _ if ctrl && key_str == "a" => Some(LineEditKey::SelectAll),
            _ if platform_clipboard == Some(Action::Paste) || (ctrl && key_str == "v") => {
                app.tab_rename_paste_clipboard();
                None
            }
            _ if ctrl || alt => None,
            _ => key_event
                .text
                .as_ref()
                .and_then(|text| text.chars().next())
                .filter(|c| !c.is_control())
                .map(LineEditKey::Char),
        };
        if let Some(key) = key {
            app.tab_rename_key(key);
        }
        return KeyOutcome::Handled;
    }

    // Menu de contexto da tab: setas escolhem, Enter executa, Esc fecha.
    if app.is_tab_menu_visible() {
        match &key_event.logical_key {
            Key::Named(NamedKey::ArrowUp) => app.move_tab_menu_selection(-1),
            Key::Named(NamedKey::ArrowDown) => app.move_tab_menu_selection(1),
            Key::Named(NamedKey::Enter) => {
                if app.activate_selected_tab_menu_item() {
                    return KeyOutcome::CloseWindow;
                }
            }
            Key::Named(NamedKey::Escape) => app.close_tab_menu(),
            _ => {}
        }
        return KeyOutcome::Handled;
    }

    // Assistente de IA aberto: as teclas vão para ele.
    if app.assistant_view().is_some() {
        use code_terminal::app::state::AssistantKey;
        let key = match &key_event.logical_key {
            Key::Named(NamedKey::Enter) => Some(AssistantKey::Enter),
            Key::Named(NamedKey::Escape) => Some(AssistantKey::Escape),
            Key::Named(NamedKey::Backspace) => Some(AssistantKey::Backspace),
            _ => key_event
                .text
                .as_ref()
                .and_then(|text| text.chars().next())
                .filter(|c| !c.is_control())
                .map(AssistantKey::Char),
        };
        if let Some(key) = key {
            app.assistant_key(key);
        }
        return KeyOutcome::Handled;
    }

    // Conexão SSH rápida: teclas vão para o seletor.
    if app.is_quick_connect_visible() {
        use code_terminal::ui::quick_connect::QuickConnectKey;
        let key = match &key_event.logical_key {
            Key::Named(NamedKey::Enter) => Some(QuickConnectKey::Enter),
            Key::Named(NamedKey::Escape) => Some(QuickConnectKey::Escape),
            Key::Named(NamedKey::Backspace) => Some(QuickConnectKey::Backspace),
            Key::Named(NamedKey::Tab) if shift => Some(QuickConnectKey::ShiftTab),
            Key::Named(NamedKey::Tab) => Some(QuickConnectKey::Tab),
            Key::Named(NamedKey::ArrowUp) => Some(QuickConnectKey::Up),
            Key::Named(NamedKey::ArrowDown) => Some(QuickConnectKey::Down),
            Key::Named(NamedKey::Space) => Some(QuickConnectKey::Space),
            _ => key_event
                .text
                .as_ref()
                .and_then(|text| text.chars().next())
                .filter(|c| !c.is_control())
                .map(QuickConnectKey::Char),
        };
        if let Some(key) = key {
            app.handle_quick_connect_key(key);
        }
        return KeyOutcome::Handled;
    }

    // Confirmação de fechar tab com programa rodando: Enter/y fecha, Esc/n
    // cancela. Tab persistente: Enter/e encerra, h/espaço só esconde.
    if app.has_pending_close_tab() {
        match key_str.as_str() {
            "enter" | "y" | "e" => {
                if app.confirm_close_pending_tab() {
                    return KeyOutcome::CloseWindow;
                }
            }
            "h" | "space" if app.pending_close_is_persistent() => {
                if app.hide_pending_close_tab() {
                    return KeyOutcome::CloseWindow;
                }
            }
            "escape" | "n" => app.cancel_close_pending_tab(),
            _ => {}
        }
        return KeyOutcome::Handled;
    }

    // Onboarding visível: qualquer tecla só fecha o aviso.
    if app.is_onboarding_visible() {
        app.dismiss_onboarding();
        return KeyOutcome::Handled;
    }

    if app.is_search_visible() {
        if search_shortcuts.search_close.matches_key_only(&key_str) {
            app.stop_search();
            return KeyOutcome::Handled;
        }
        if search_shortcuts
            .search_next
            .matches(ctrl, shift, alt, &key_str)
        {
            app.next_search_match();
            return KeyOutcome::Handled;
        }
        if search_shortcuts
            .search_previous
            .matches(ctrl, shift, alt, &key_str)
        {
            app.previous_search_match();
            return KeyOutcome::Handled;
        }
    }

    if search_shortcuts
        .search_open
        .matches(ctrl, shift, alt, &key_str)
    {
        app.start_search();
        return KeyOutcome::Handled;
    }

    // Command palette toggle (Ctrl+Shift+P)
    if ctrl && shift && key_str == "p" {
        app.toggle_command_palette();
        return KeyOutcome::Handled;
    }

    if app.is_command_palette_visible() {
        return handle_palette_key(key_event, app, event_proxy);
    }

    // Tela de configurações aberta: todas as teclas são dela (nenhuma
    // escapa para o programa do terminal).
    if app.is_settings_visible() {
        let capturing = app
            .settings_screen()
            .is_some_and(|screen| screen.is_capturing());
        let input_key = match key_event.physical_key {
            // Esperando a tecla de um atalho: Esc cancela, o resto vira o atalho.
            PhysicalKey::Code(KeyCode::Escape) => Some(SettingsInputKey::Escape),
            _ if capturing => {
                code_terminal::ui::settings::binding_from_key(ctrl, shift, alt, &key_str)
                    .map(SettingsInputKey::Captured)
            }
            PhysicalKey::Code(KeyCode::Enter) => Some(SettingsInputKey::Enter),
            PhysicalKey::Code(KeyCode::ArrowUp) => Some(SettingsInputKey::Up),
            PhysicalKey::Code(KeyCode::ArrowDown) => Some(SettingsInputKey::Down),
            PhysicalKey::Code(KeyCode::ArrowLeft) => Some(SettingsInputKey::Left),
            PhysicalKey::Code(KeyCode::ArrowRight) => Some(SettingsInputKey::Right),
            PhysicalKey::Code(KeyCode::Tab) if shift => Some(SettingsInputKey::PreviousSection),
            PhysicalKey::Code(KeyCode::Tab) => Some(SettingsInputKey::NextSection),
            PhysicalKey::Code(KeyCode::Backspace) => Some(SettingsInputKey::Backspace),
            _ if ctrl || alt => None,
            _ => key_event
                .text
                .as_ref()
                .and_then(|text| text.chars().next())
                .map(SettingsInputKey::Char),
        };
        if let Some(input_key) = input_key {
            app.handle_settings_key(input_key);
        }
        return KeyOutcome::Handled;
    }

    // Cópia nativa da plataforma ou pelo atalho configurado.
    if platform_clipboard == Some(Action::Copy)
        || clipboard_shortcuts.copy.matches(ctrl, shift, alt, &key_str)
    {
        app.copy_selection_and_notify();
        return KeyOutcome::Handled;
    }

    // Colagem nativa da plataforma, pelo atalho configurado ou por Shift+Insert.
    let shift_insert =
        shift && !ctrl && !alt && key_event.physical_key == PhysicalKey::Code(KeyCode::Insert);
    if platform_clipboard == Some(Action::Paste)
        || shift_insert
        || clipboard_shortcuts
            .paste
            .matches(ctrl, shift, alt, &key_str)
    {
        app.paste_and_notify(false);
        return KeyOutcome::Handled;
    }

    // Atalhos de tabs com os modificadores reais (antes os modificadores
    // eram fixos no código e o atalho configurado era ignorado).
    if tab_shortcuts.next_tab.matches(ctrl, shift, alt, &key_str) {
        app.next_tab();
        return KeyOutcome::Handled;
    }
    if tab_shortcuts
        .previous_tab
        .matches(ctrl, shift, alt, &key_str)
    {
        app.previous_tab();
        return KeyOutcome::Handled;
    }
    if tab_shortcuts.new_tab.matches(ctrl, shift, alt, &key_str) {
        if let Err(e) = app.create_new_tab() {
            tracing::error!("Failed to create new tab: {}", e);
        }
        return KeyOutcome::Handled;
    }
    if tab_shortcuts.close_tab.matches(ctrl, shift, alt, &key_str) {
        if app.close_active_tab() {
            tracing::info!("Last tab closed, closing window");
            return KeyOutcome::CloseWindow;
        }
        return KeyOutcome::Handled;
    }
    if ctrl && !shift && !alt {
        if let Some(tab_index) = tab_shortcuts.is_go_to_tab(true, false, false, &key_str) {
            // Ctrl+N conta as linhas da sidebar (grupo recolhido é uma linha).
            app.go_to_visible_tab(tab_index);
            return KeyOutcome::Handled;
        }
    }

    // Sidebar e avisos.
    let matches_binding = |binding: &str| {
        code_terminal::config::keybindings::Shortcut::parse(binding)
            .is_some_and(|shortcut| shortcut.matches(ctrl, shift, alt, &key_str))
    };
    // F2 renomeia a tab ativa quando a sidebar está à vista; com ela
    // escondida, a tecla segue para o programa (htop, mc usam F2).
    if app.is_sidebar_visible() && matches_binding(&app.config.keybindings.rename_tab) {
        app.start_rename_active_tab();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.toggle_sidebar) {
        app.toggle_sidebar();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.open_settings) {
        app.show_settings();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.jump_to_unread) {
        app.jump_to_unread();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.show_notifications) {
        app.toggle_notifications_panel();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.toggle_inspector) {
        app.toggle_inspector();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.ai_explain) {
        app.ai_explain_output();
        return KeyOutcome::Handled;
    }
    if matches_binding(&app.config.keybindings.ai_suggest) {
        app.ai_open_suggest();
        return KeyOutcome::Handled;
    }
    if app.is_notifications_panel_visible() && key_str == "escape" {
        app.toggle_notifications_panel();
        return KeyOutcome::Handled;
    }

    // Painéis: dividir, fechar, navegar e redimensionar.
    if let Some(action) = NativeSplitShortcuts::from_config(&app.config.keybindings.splits)
        .try_get_action(ctrl, shift, alt, &key_str)
    {
        return handle_split_action(app, action);
    }

    // Ctrl+Shift+N - New Window
    if ctrl && shift && key_str == "n" {
        if let Err(e) = event_proxy.send_event(WindowEventMsg::CreateWindow {
            position: None,
            size: (900, 600),
        }) {
            tracing::warn!("Failed to request new window: {}", e);
        }
        return KeyOutcome::Handled;
    }

    // Shift+PageUp/PageDown rolam o histórico sem mandar nada ao programa.
    if shift && !ctrl && !alt {
        if let PhysicalKey::Code(code @ (KeyCode::PageUp | KeyCode::PageDown)) =
            key_event.physical_key
        {
            let page = app.terminal_size.1.saturating_sub(1).max(1) as usize;
            if let Some(grid) = app.active_tab_mut().and_then(|tab| tab.grid_mut()) {
                if code == KeyCode::PageUp {
                    grid.scroll_up_lines(page);
                } else {
                    grid.scroll_down_lines(page);
                }
            }
            return KeyOutcome::Handled;
        }
    }

    // Busca ativa: o texto digitado vai para a consulta.
    if app.is_search_active() {
        match key_event.physical_key {
            PhysicalKey::Code(KeyCode::Enter) => {
                app.next_search_match();
                return KeyOutcome::Handled;
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                let mut query = app.get_search_query();
                query.pop();
                app.update_search_query(&query);
                return KeyOutcome::Handled;
            }
            _ => {
                if let Some(text) = key_event.text.as_ref().filter(|t| !t.is_empty() && !ctrl) {
                    let new_query = format!("{}{}", app.get_search_query(), text);
                    app.update_search_query(&new_query);
                    return KeyOutcome::Handled;
                }
            }
        }
    }

    // Todo o resto vai para o programa no terminal.
    let app_cursor = app
        .active_tab_grid()
        .is_some_and(|grid| grid.modes().cursor_keys == CursorKeysMode::Application);
    let key_mods = KeyMods { shift, alt, ctrl };
    if let Some(bytes) = encode_key(
        &key_event.logical_key,
        key_event.text.as_deref(),
        key_mods,
        app_cursor,
    ) {
        // Digitar volta a visão para o fim do histórico.
        if let Some(grid) = app.active_tab_mut().and_then(|tab| tab.grid_mut()) {
            if grid.scroll_offset() > 0 {
                grid.scroll_to_bottom();
            }
        }
        // Plugins veem o texto digitado e podem consumi-lo.
        if app.plugin_input(&String::from_utf8_lossy(&bytes)) {
            return KeyOutcome::Handled;
        }
        write_to_active_terminal(app, &bytes, "key input");
    }
    KeyOutcome::Handled
}

fn handle_palette_key(
    key_event: &KeyEvent,
    app: &mut AppState,
    event_proxy: &EventLoopProxy<WindowEventMsg>,
) -> KeyOutcome {
    let input_key = match key_event.physical_key {
        PhysicalKey::Code(KeyCode::Enter) => Some(PaletteInputKey::Enter),
        PhysicalKey::Code(KeyCode::Escape) => Some(PaletteInputKey::Escape),
        PhysicalKey::Code(KeyCode::Backspace) => Some(PaletteInputKey::Backspace),
        PhysicalKey::Code(KeyCode::ArrowUp) => Some(PaletteInputKey::Up),
        PhysicalKey::Code(KeyCode::ArrowDown) => Some(PaletteInputKey::Down),
        _ => key_event
            .text
            .as_ref()
            .and_then(|text| text.chars().next())
            .map(PaletteInputKey::Char),
    };
    let Some(input_key) = input_key else {
        return KeyOutcome::Handled;
    };
    run_palette_input(&input_key, app, event_proxy)
}

/// Aplica uma tecla à paleta e executa a ação que ela devolver. O clique num
/// item passa por aqui com Enter, para ter o mesmo efeito do teclado.
fn run_palette_input(
    input_key: &PaletteInputKey,
    app: &mut AppState,
    event_proxy: &EventLoopProxy<WindowEventMsg>,
) -> KeyOutcome {
    match app.handle_command_palette_key(input_key) {
        PaletteAction::NewTab => {
            if let Err(e) = app.create_new_tab() {
                tracing::error!("Failed to create new tab: {}", e);
            }
        }
        PaletteAction::CloseTab => {
            if app.close_active_tab() {
                return KeyOutcome::CloseWindow;
            }
        }
        PaletteAction::NextTab => app.next_tab(),
        PaletteAction::PreviousTab => app.previous_tab(),
        PaletteAction::NewWindow => {
            if let Err(e) = event_proxy.send_event(WindowEventMsg::CreateWindow {
                position: None,
                size: (900, 600),
            }) {
                tracing::warn!("Failed to request new window: {}", e);
            }
        }
        PaletteAction::SplitHorizontal => split_pane(app, SplitDirection::Horizontal),
        PaletteAction::SplitVertical => split_pane(app, SplitDirection::Vertical),
        PaletteAction::Search => app.start_search(),
        PaletteAction::ToggleSidebar => app.toggle_sidebar(),
        PaletteAction::RenameTab => {
            app.start_rename_active_tab();
        }
        PaletteAction::JumpToUnread => {
            app.jump_to_unread();
        }
        PaletteAction::ShowNotifications => app.toggle_notifications_panel(),
        PaletteAction::ToggleInspector => app.toggle_inspector(),
        PaletteAction::AiExplain => app.ai_explain_output(),
        PaletteAction::AiSuggest => app.ai_open_suggest(),
        PaletteAction::OpenBrowser => app.open_browser_window(),
        PaletteAction::NewPersistentTab => match app.open_persistent_tab() {
            Ok(name) => tracing::info!(session = %name, "Persistent tab opened"),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to open persistent tab");
                app.show_toast(
                    code_terminal::ui::i18n::persistent_failed(true, &e.to_string()),
                    code_terminal::ui::toast::ToastKind::Error,
                );
            }
        },
        PaletteAction::TogglePersistent => {
            let index = app.active_tab_index();
            app.toggle_tab_persistent_notify(index);
        }
        PaletteAction::BackgroundSessions => {
            app.show_all_background_sessions();
        }
        PaletteAction::ToggleTheme => {
            let theme = app.toggle_theme().to_string();
            tracing::info!(theme = %theme, "Theme changed");
        }
        PaletteAction::QuickConnectSsh => app.show_quick_connect(),
        PaletteAction::OpenSettings => app.show_settings(),
        PaletteAction::ChooseTheme => app.open_theme_chooser(),
        PaletteAction::Quit => return KeyOutcome::CloseWindow,
        PaletteAction::None => {}
    }
    KeyOutcome::Handled
}

/// Aplica ao painel aberto a decisão do mouse ([`overlay_hits`]).
///
/// Devolve `None` quando nenhum painel está aberto (o evento segue para a
/// barra de tabs e o terminal) e `Some` quando o painel ficou com ele.
fn apply_overlay_mouse(
    route: OverlayMouse,
    app: &mut AppState,
    event_proxy: &EventLoopProxy<WindowEventMsg>,
) -> Option<KeyOutcome> {
    match route {
        OverlayMouse::Pass => return None,
        OverlayMouse::Consume => {}
        OverlayMouse::Close(kind) => app.close_overlay(kind),
        OverlayMouse::Hover(kind, index) => app.hover_overlay_item(kind, index),
        OverlayMouse::Scroll(kind, lines) => app.scroll_overlay(kind, lines),
        OverlayMouse::Activate(OverlayKind::CommandPalette, index) => {
            app.command_palette_mut().select(index);
            return Some(run_palette_input(&PaletteInputKey::Enter, app, event_proxy));
        }
        OverlayMouse::Activate(OverlayKind::QuickConnect, index) => {
            app.activate_quick_connect_item(index);
        }
        OverlayMouse::Activate(OverlayKind::Settings, index) => {
            app.activate_settings_row(index);
        }
        OverlayMouse::Activate(OverlayKind::TabMenu, index) => {
            if app.activate_tab_menu_item(index) {
                return Some(KeyOutcome::CloseWindow);
            }
        }
        OverlayMouse::Activate(_, _) => {}
    }
    Some(KeyOutcome::Handled)
}

/// Mouse com um painel aberto: decide pelo mapa do último frame e aplica.
///
/// `None` quando não há painel aberto (ou ele já fechou desde o último
/// desenho): o evento segue o caminho normal.
fn overlay_mouse_event(
    window: &mut Window,
    route: impl FnOnce(&overlay_hits::OverlayHits) -> OverlayMouse,
    event_proxy: &EventLoopProxy<WindowEventMsg>,
) -> Option<KeyOutcome> {
    let app = window.app_state.as_mut()?;
    let top = window.overlay_hits.top()?;
    if !app.is_overlay_visible(top.kind) {
        return None;
    }
    let decision = route(&window.overlay_hits);
    apply_overlay_mouse(decision, app, event_proxy)
}

/// Linhas que um evento de roda representa (positivo = para baixo, como a lista).
fn wheel_lines(delta: &MouseScrollDelta) -> i32 {
    let (amount, step) = match delta {
        MouseScrollDelta::LineDelta(_, y) => (f64::from(*y), 1.0),
        MouseScrollDelta::PixelDelta(position) => (position.y, 20.0),
    };
    if amount == 0.0 {
        return 0;
    }
    // Roda de alta resolução e touchpad mandam frações: andam ao menos um item.
    let lines = (amount / step).round() as i32;
    let lines = if lines == 0 {
        amount.signum() as i32
    } else {
        lines
    };
    -lines
}

fn write_to_active_terminal(app: &mut AppState, data: &[u8], context: &str) {
    if let Err(e) = app.write_to_active_tab(data) {
        tracing::warn!(context, error = %e, "Failed to write to active terminal");
    }
}

/// Divide o painel em foco.
fn split_pane(app: &mut AppState, direction: SplitDirection) {
    if let Err(e) = app.split_active_pane(direction) {
        tracing::error!(error = %e, "Failed to split pane");
    }
}

/// Executa uma ação de painel vinda de um atalho.
fn handle_split_action(app: &mut AppState, action: Action) -> KeyOutcome {
    match action {
        Action::SplitHorizontal => split_pane(app, SplitDirection::Horizontal),
        Action::SplitVertical => split_pane(app, SplitDirection::Vertical),
        Action::ClosePane => {
            if app.close_active_pane() {
                return KeyOutcome::CloseWindow;
            }
        }
        Action::NavigateUp => {
            app.focus_pane_direction(FocusDirection::Up);
        }
        Action::NavigateDown => {
            app.focus_pane_direction(FocusDirection::Down);
        }
        Action::NavigateLeft => {
            app.focus_pane_direction(FocusDirection::Left);
        }
        Action::NavigateRight => {
            app.focus_pane_direction(FocusDirection::Right);
        }
        Action::NextPane => {
            app.cycle_pane(true);
        }
        Action::PreviousPane => {
            app.cycle_pane(false);
        }
        Action::IncreasePaneSize => {
            app.resize_active_pane(true);
        }
        Action::DecreasePaneSize => {
            app.resize_active_pane(false);
        }
        _ => {}
    }
    KeyOutcome::Handled
}

fn report_button(button: winit::event::MouseButton) -> Option<ReportButton> {
    match button {
        winit::event::MouseButton::Left => Some(ReportButton::Left),
        winit::event::MouseButton::Middle => Some(ReportButton::Middle),
        winit::event::MouseButton::Right => Some(ReportButton::Right),
        _ => None,
    }
}

/// Envia o evento ao programa se ele ligou um modo de mouse (e Shift não está
/// segurado). Devolve `true` quando o evento é do programa, não do terminal.
fn report_mouse(
    app: &mut AppState,
    action: MouseAction,
    (row, col): (usize, usize),
    modifiers: &Modifiers,
) -> bool {
    let Some(modes) = app
        .active_tab()
        .and_then(|tab| tab.grid())
        .map(|grid| grid.modes().clone())
    else {
        return false;
    };
    let state = modifiers.state();
    if !mouse_report::reporting_active(&modes, state.shift_key()) {
        return false;
    }
    let mods = MouseMods {
        shift: state.shift_key(),
        alt: state.alt_key(),
        ctrl: state.control_key(),
    };
    if let Some(bytes) = mouse_report::encode_mouse(&modes, action, row, col, mods) {
        write_to_active_terminal(app, &bytes, "mouse report");
    }
    true
}

/// Rodinha: vai para o programa (modo de mouse), vira setas na tela
/// alternativa (less, man) ou rola o histórico.
fn handle_mouse_wheel(
    delta: &MouseScrollDelta,
    app: &mut AppState,
    cell: (usize, usize),
    modifiers: &Modifiers,
) {
    const LINES_PER_TICK: usize = 3;

    let (ticks, up) = match delta {
        MouseScrollDelta::LineDelta(_, y) => ((y.abs().round() as usize).max(1), *y > 0.0),
        MouseScrollDelta::PixelDelta(pos) => ((pos.y.abs() as usize / 20).max(1), pos.y > 0.0),
    };
    if matches!(delta, MouseScrollDelta::LineDelta(_, y) if *y == 0.0)
        || matches!(delta, MouseScrollDelta::PixelDelta(pos) if pos.y == 0.0)
    {
        return;
    }

    let action = if up {
        MouseAction::WheelUp
    } else {
        MouseAction::WheelDown
    };
    if report_mouse(app, action, cell, modifiers) {
        for _ in 1..ticks {
            report_mouse(app, action, cell, modifiers);
        }
        return;
    }

    let arrows = app
        .active_tab()
        .and_then(|tab| tab.grid())
        .and_then(|grid| mouse_report::wheel_as_arrows(grid.modes(), up, ticks * LINES_PER_TICK));
    if let Some(bytes) = arrows {
        write_to_active_terminal(app, &bytes, "wheel as arrows");
        return;
    }

    if let Some(grid) = app.active_tab_mut().and_then(|tab| tab.grid_mut()) {
        if up {
            grid.scroll_up_lines(ticks * LINES_PER_TICK);
        } else {
            grid.scroll_down_lines(ticks * LINES_PER_TICK);
        }
    }
}

/// Converte PhysicalKey para string (para matching de atalhos)
fn physical_key_to_string(key: &PhysicalKey) -> String {
    match key {
        PhysicalKey::Code(KeyCode::KeyA) => "a".to_string(),
        PhysicalKey::Code(KeyCode::KeyB) => "b".to_string(),
        PhysicalKey::Code(KeyCode::KeyC) => "c".to_string(),
        PhysicalKey::Code(KeyCode::KeyD) => "d".to_string(),
        PhysicalKey::Code(KeyCode::KeyE) => "e".to_string(),
        PhysicalKey::Code(KeyCode::KeyF) => "f".to_string(),
        PhysicalKey::Code(KeyCode::KeyG) => "g".to_string(),
        PhysicalKey::Code(KeyCode::KeyH) => "h".to_string(),
        PhysicalKey::Code(KeyCode::KeyI) => "i".to_string(),
        PhysicalKey::Code(KeyCode::KeyJ) => "j".to_string(),
        PhysicalKey::Code(KeyCode::KeyK) => "k".to_string(),
        PhysicalKey::Code(KeyCode::KeyL) => "l".to_string(),
        PhysicalKey::Code(KeyCode::KeyM) => "m".to_string(),
        PhysicalKey::Code(KeyCode::KeyN) => "n".to_string(),
        PhysicalKey::Code(KeyCode::KeyO) => "o".to_string(),
        PhysicalKey::Code(KeyCode::KeyP) => "p".to_string(),
        PhysicalKey::Code(KeyCode::KeyQ) => "q".to_string(),
        PhysicalKey::Code(KeyCode::KeyR) => "r".to_string(),
        PhysicalKey::Code(KeyCode::KeyS) => "s".to_string(),
        PhysicalKey::Code(KeyCode::KeyT) => "t".to_string(),
        PhysicalKey::Code(KeyCode::KeyU) => "u".to_string(),
        PhysicalKey::Code(KeyCode::KeyV) => "v".to_string(),
        PhysicalKey::Code(KeyCode::KeyW) => "w".to_string(),
        PhysicalKey::Code(KeyCode::KeyX) => "x".to_string(),
        PhysicalKey::Code(KeyCode::KeyY) => "y".to_string(),
        PhysicalKey::Code(KeyCode::KeyZ) => "z".to_string(),
        PhysicalKey::Code(KeyCode::Digit0) => "0".to_string(),
        PhysicalKey::Code(KeyCode::Digit1) => "1".to_string(),
        PhysicalKey::Code(KeyCode::Digit2) => "2".to_string(),
        PhysicalKey::Code(KeyCode::Digit3) => "3".to_string(),
        PhysicalKey::Code(KeyCode::Digit4) => "4".to_string(),
        PhysicalKey::Code(KeyCode::Digit5) => "5".to_string(),
        PhysicalKey::Code(KeyCode::Digit6) => "6".to_string(),
        PhysicalKey::Code(KeyCode::Digit7) => "7".to_string(),
        PhysicalKey::Code(KeyCode::Digit8) => "8".to_string(),
        PhysicalKey::Code(KeyCode::Digit9) => "9".to_string(),
        PhysicalKey::Code(KeyCode::Tab) => "tab".to_string(),
        PhysicalKey::Code(KeyCode::Escape) => "escape".to_string(),
        PhysicalKey::Code(KeyCode::F1) => "f1".to_string(),
        PhysicalKey::Code(KeyCode::F2) => "f2".to_string(),
        PhysicalKey::Code(KeyCode::F3) => "f3".to_string(),
        PhysicalKey::Code(KeyCode::F4) => "f4".to_string(),
        PhysicalKey::Code(KeyCode::F5) => "f5".to_string(),
        PhysicalKey::Code(KeyCode::F6) => "f6".to_string(),
        PhysicalKey::Code(KeyCode::F7) => "f7".to_string(),
        PhysicalKey::Code(KeyCode::F8) => "f8".to_string(),
        PhysicalKey::Code(KeyCode::F9) => "f9".to_string(),
        PhysicalKey::Code(KeyCode::F10) => "f10".to_string(),
        PhysicalKey::Code(KeyCode::F11) => "f11".to_string(),
        PhysicalKey::Code(KeyCode::F12) => "f12".to_string(),
        PhysicalKey::Code(KeyCode::ArrowUp) => "up".to_string(),
        PhysicalKey::Code(KeyCode::ArrowDown) => "down".to_string(),
        PhysicalKey::Code(KeyCode::ArrowLeft) => "left".to_string(),
        PhysicalKey::Code(KeyCode::ArrowRight) => "right".to_string(),
        PhysicalKey::Code(KeyCode::PageUp) => "pageup".to_string(),
        PhysicalKey::Code(KeyCode::PageDown) => "pagedown".to_string(),
        PhysicalKey::Code(KeyCode::Home) => "home".to_string(),
        PhysicalKey::Code(KeyCode::End) => "end".to_string(),
        PhysicalKey::Code(KeyCode::Minus) => "minus".to_string(),
        PhysicalKey::Code(KeyCode::Equal) => "equal".to_string(),
        PhysicalKey::Code(KeyCode::BracketLeft) => "bracketleft".to_string(),
        PhysicalKey::Code(KeyCode::BracketRight) => "bracketright".to_string(),
        PhysicalKey::Code(KeyCode::Comma) => "comma".to_string(),
        PhysicalKey::Code(KeyCode::Period) => "period".to_string(),
        PhysicalKey::Code(KeyCode::Space) => "space".to_string(),
        PhysicalKey::Code(KeyCode::Enter) => "enter".to_string(),
        _ => "".to_string(),
    }
}

#[cfg(test)]
mod clipboard_shortcut_tests {
    use super::*;
    use code_terminal::terminal::grid::{Grid, SelectionKind};

    #[test]
    fn macos_uses_command_for_the_clipboard() {
        let platform = ClipboardPlatform::Macos;
        assert_eq!(
            platform.shortcut(ModifiersState::SUPER, "c", None),
            Some(Action::Copy)
        );
        assert_eq!(
            platform.shortcut(ModifiersState::SUPER, "v", None),
            Some(Action::Paste)
        );
        assert_eq!(platform.shortcut(ModifiersState::CONTROL, "c", None), None);
        assert_eq!(platform.shortcut(ModifiersState::CONTROL, "v", None), None);
        assert_eq!(
            platform.shortcut(ModifiersState::SUPER | ModifiersState::SHIFT, "v", None),
            None
        );
    }

    #[test]
    fn windows_copies_only_when_text_is_selected() {
        let platform = ClipboardPlatform::Windows;
        let mut grid = Grid::new(80, 24);
        let ctrl = ModifiersState::CONTROL;

        assert_eq!(platform.shortcut(ctrl, "c", Some(&grid)), None);
        grid.start_selection(0, 0, SelectionKind::Normal);
        assert_eq!(platform.shortcut(ctrl, "c", Some(&grid)), None);
        grid.update_selection(0, 4);
        assert_eq!(
            platform.shortcut(ctrl, "c", Some(&grid)),
            Some(Action::Copy)
        );
    }

    #[test]
    fn windows_pastes_on_the_main_screen_and_preserves_ctrl_v_on_the_alternate_screen() {
        let platform = ClipboardPlatform::Windows;
        let mut grid = Grid::new(80, 24);
        let ctrl = ModifiersState::CONTROL;

        assert_eq!(
            platform.shortcut(ctrl, "v", Some(&grid)),
            Some(Action::Paste)
        );
        grid.enable_alternate_buffer();
        assert_eq!(platform.shortcut(ctrl, "v", Some(&grid)), None);
    }

    #[test]
    fn linux_keeps_the_configured_clipboard_shortcuts() {
        let platform = ClipboardPlatform::Other;
        let grid = Grid::new(80, 24);
        assert_eq!(
            platform.shortcut(ModifiersState::CONTROL, "v", Some(&grid)),
            None
        );
        assert_eq!(
            platform.shortcut(ModifiersState::SUPER, "v", Some(&grid)),
            None
        );
    }
}
