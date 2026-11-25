//! Processo `code-terminal-browser`: a janela do navegador e o servidor que
//! recebe os pedidos do terminal (ver [`crate::browser::remote`]).
//!
//! Linux: GTK + WebKitGTK (funciona no Wayland, onde o wry não embute um
//! WebView numa janela winit). Windows/macOS: janela winit + wry.

use crate::browser::remote::{
    eval_result_text, normalize_url, BrowserReply, BrowserRequest, Envelope, TEXT_SCRIPT,
};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// Pedido vindo do socket, com o canal da resposta.
pub type Job = (BrowserRequest, Sender<BrowserReply>);

/// Tempo máximo esperando a janela responder (ex.: `eval` lento).
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);

/// Navegação permitida dentro da janela (links clicados, redirecionamentos).
pub fn navigation_allowed(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https" | "file" | "about" | "blob"))
        .unwrap_or(false)
}

/// Atende uma conexão: um pedido por linha, uma resposta por linha.
pub fn serve_connection(stream: impl std::io::Read + Write, token: &str, jobs: &Sender<Job>) {
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let reply = match serde_json::from_str::<Envelope>(line.trim()) {
            Err(e) => BrowserReply::error(format!("invalid request: {e}")),
            Ok(envelope) if envelope.token != token => BrowserReply::error("invalid token"),
            Ok(envelope) => dispatch(jobs, envelope.request),
        };
        let Ok(mut json) = serde_json::to_string(&reply) else {
            return;
        };
        json.push('\n');
        let stream = reader.get_mut();
        if stream.write_all(json.as_bytes()).is_err() || stream.flush().is_err() {
            return;
        }
    }
}

fn dispatch(jobs: &Sender<Job>, request: BrowserRequest) -> BrowserReply {
    let (tx, rx) = mpsc::channel();
    if jobs.send((request, tx)).is_err() {
        return BrowserReply::error("browser window is closing");
    }
    rx.recv_timeout(REPLY_TIMEOUT)
        .unwrap_or_else(|_| BrowserReply::error("browser did not answer in time"))
}

/// Escuta o socket e repassa os pedidos para a thread da janela.
fn start_server(jobs: Sender<Job>) -> Result<ServerGuard, String> {
    #[cfg(unix)]
    {
        let path = crate::browser::remote::default_browser_socket();
        let listener = crate::multiplexer::daemon::bind_socket(&path).map_err(|e| e.to_string())?;
        std::thread::Builder::new()
            .name("browser-socket".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let jobs = jobs.clone();
                    let _ = std::thread::Builder::new()
                        .name("browser-client".into())
                        .spawn(move || serve_connection(stream, "", &jobs));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(ServerGuard { path })
    }
    #[cfg(not(unix))]
    {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let path = crate::browser::remote::default_browser_endpoint_file();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, format!("{port} {token}")).map_err(|e| e.to_string())?;
        std::thread::Builder::new()
            .name("browser-socket".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let jobs = jobs.clone();
                    let token = token.clone();
                    let _ = std::thread::Builder::new()
                        .name("browser-client".into())
                        .spawn(move || serve_connection(stream, &token, &jobs));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(ServerGuard { path })
    }
}

/// Apaga o socket (ou o arquivo de porta) quando a janela fecha.
struct ServerGuard {
    path: std::path::PathBuf,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// O que a janela sabe fazer; cada plataforma implementa.
trait BrowserWindow {
    fn webview(&self) -> &wry::WebView;
    fn title(&self) -> Option<String>;
    fn focus(&self);
    fn quit(&self);
}

/// Executa um pedido na thread da janela.
fn apply(window: &dyn BrowserWindow, request: BrowserRequest, reply: Sender<BrowserReply>) {
    let webview = window.webview();
    let result = match request {
        BrowserRequest::Open { url } => match normalize_url(&url) {
            Ok(url) => webview.load_url(&url).map(|_| Some(url)),
            Err(e) => {
                let _ = reply.send(BrowserReply::error(e));
                return;
            }
        },
        BrowserRequest::Back => webview.evaluate_script("history.back()").map(|_| None),
        BrowserRequest::Forward => webview.evaluate_script("history.forward()").map(|_| None),
        BrowserRequest::Reload => webview.reload().map(|_| None),
        BrowserRequest::Eval { js } => {
            eval_async(webview, &js, reply, false);
            return;
        }
        BrowserRequest::Text => {
            eval_async(webview, TEXT_SCRIPT, reply, true);
            return;
        }
        BrowserRequest::Info => Ok(None),
        BrowserRequest::Focus => {
            window.focus();
            Ok(None)
        }
        BrowserRequest::Close => {
            let _ = reply.send(BrowserReply::ok());
            window.quit();
            return;
        }
    };
    let answer = match result {
        Ok(opened) => BrowserReply {
            ok: true,
            url: opened.or_else(|| webview.url().ok()),
            title: window.title(),
            ..BrowserReply::default()
        },
        Err(e) => BrowserReply::error(e.to_string()),
    };
    let _ = reply.send(answer);
}

/// `eval` com resposta: o WebView chama de volta com o resultado em JSON.
fn eval_async(webview: &wry::WebView, js: &str, reply: Sender<BrowserReply>, as_text: bool) {
    let callback_reply = reply.clone();
    let sent = webview.evaluate_script_with_callback(js, move |json| {
        let result = if as_text {
            eval_result_text(&json)
        } else {
            json
        };
        let _ = callback_reply.send(BrowserReply {
            ok: true,
            result: Some(result),
            ..BrowserReply::default()
        });
    });
    if let Err(e) = sent {
        let _ = reply.send(BrowserReply::error(e.to_string()));
    }
}

/// Esvazia a fila de pedidos (chamado periodicamente pela janela).
fn drain(window: &dyn BrowserWindow, jobs: &Receiver<Job>) {
    while let Ok((request, reply)) = jobs.try_recv() {
        apply(window, request, reply);
    }
}

const WINDOW_TITLE: &str = "Code Terminal Browser";

/// Abre a janela em `initial_url` e atende pedidos até ela fechar.
pub fn run(initial_url: &str) -> Result<(), String> {
    let url = normalize_url(initial_url)?;
    // Antes de qualquer thread: muda variável de ambiente.
    #[cfg(target_os = "linux")]
    platform::avoid_nvidia_dmabuf_crash();
    let (tx, rx) = mpsc::channel();
    let _server = start_server(tx)?;
    platform::run(&url, rx)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use gtk::glib;
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use wry::{WebViewBuilder, WebViewBuilderExtUnix};

    struct GtkBrowser {
        window: gtk::Window,
        webview: wry::WebView,
        title: Rc<RefCell<Option<String>>>,
    }

    impl BrowserWindow for GtkBrowser {
        fn webview(&self) -> &wry::WebView {
            &self.webview
        }
        fn title(&self) -> Option<String> {
            self.title.borrow().clone()
        }
        fn focus(&self) {
            self.window.present();
        }
        fn quit(&self) {
            gtk::main_quit();
        }
    }

    /// Com o driver da NVIDIA, o renderizador DMA-BUF do WebKitGTK derruba a
    /// conexão com o Wayland ("Error 71 (Protocol error) dispatching to
    /// Wayland display") e a janela fecha ao abrir. Desliga só ele, e só se o
    /// usuário não escolheu outra coisa.
    pub(super) fn avoid_nvidia_dmabuf_crash() {
        let nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists();
        if nvidia && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    pub fn run(url: &str, jobs: Receiver<Job>) -> Result<(), String> {
        // app_id no Wayland / WM_CLASS no X11 (regras de janela do usuário).
        glib::set_prgname(Some("code-terminal-browser"));
        gtk::init().map_err(|e| format!("GTK init failed: {e}"))?;
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title(WINDOW_TITLE);
        window.set_default_size(1100, 800);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&container);

        let title = Rc::new(RefCell::new(None));
        let title_slot = Rc::clone(&title);
        let title_window = window.clone();
        let webview = WebViewBuilder::new()
            .with_url(url)
            .with_navigation_handler(|target| navigation_allowed(&target))
            .with_document_title_changed_handler(move |page_title| {
                title_window.set_title(&format!("{page_title} — {WINDOW_TITLE}"));
                *title_slot.borrow_mut() = Some(page_title);
            })
            .build_gtk(&container)
            .map_err(|e| format!("WebView failed: {e}"))?;
        window.show_all();
        window.connect_delete_event(|_, _| {
            gtk::main_quit();
            glib::Propagation::Proceed
        });

        let browser = GtkBrowser {
            window,
            webview,
            title,
        };
        glib::timeout_add_local(Duration::from_millis(20), move || {
            drain(&browser, &jobs);
            glib::ControlFlow::Continue
        });
        gtk::main();
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};
    use wry::WebViewBuilder;

    struct WinitBrowser {
        window: Window,
        webview: wry::WebView,
        title: Rc<RefCell<Option<String>>>,
        quit: Rc<RefCell<bool>>,
    }

    impl BrowserWindow for WinitBrowser {
        fn webview(&self) -> &wry::WebView {
            &self.webview
        }
        fn title(&self) -> Option<String> {
            self.title.borrow().clone()
        }
        fn focus(&self) {
            self.window.focus_window();
        }
        fn quit(&self) {
            *self.quit.borrow_mut() = true;
        }
    }

    struct App {
        url: String,
        jobs: Receiver<Job>,
        browser: Option<WinitBrowser>,
        error: Option<String>,
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.browser.is_some() {
                return;
            }
            match open_window(event_loop, &self.url) {
                Ok(browser) => self.browser = Some(browser),
                Err(e) => {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if let WindowEvent::CloseRequested = event {
                event_loop.exit();
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(browser) = &self.browser {
                drain(browser, &self.jobs);
                if *browser.quit.borrow() {
                    event_loop.exit();
                }
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                std::time::Instant::now() + Duration::from_millis(20),
            ));
        }
    }

    fn open_window(event_loop: &ActiveEventLoop, url: &str) -> Result<WinitBrowser, String> {
        let attributes = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 800.0));
        let window = event_loop
            .create_window(attributes)
            .map_err(|e| e.to_string())?;
        let title = Rc::new(RefCell::new(None));
        let title_slot = Rc::clone(&title);
        let webview = WebViewBuilder::new()
            .with_url(url)
            .with_navigation_handler(|target| navigation_allowed(&target))
            .with_document_title_changed_handler(move |page_title| {
                *title_slot.borrow_mut() = Some(page_title);
            })
            .build(&window)
            .map_err(|e| format!("WebView failed: {e}"))?;
        Ok(WinitBrowser {
            window,
            webview,
            title,
            quit: Rc::new(RefCell::new(false)),
        })
    }

    pub fn run(url: &str, jobs: Receiver<Job>) -> Result<(), String> {
        let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
        let mut app = App {
            url: url.to_string(),
            jobs,
            browser: None,
            error: None,
        };
        event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
        app.error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_allowed() {
        assert!(navigation_allowed("https://example.com/a"));
        assert!(navigation_allowed("file:///tmp/x.html"));
        assert!(!navigation_allowed("javascript:alert(1)"));
        assert!(!navigation_allowed("ftp://x"));
        assert!(!navigation_allowed("lixo"));
    }

    /// O servidor confere o token e repassa o pedido para a "janela".
    #[test]
    fn test_serve_connection_checks_token_and_dispatches() {
        let (tx, rx) = mpsc::channel::<Job>();
        let window = std::thread::spawn(move || {
            while let Ok((request, reply)) = rx.recv() {
                let answer = match request {
                    BrowserRequest::Info => BrowserReply {
                        ok: true,
                        url: Some("about:blank".into()),
                        ..BrowserReply::default()
                    },
                    other => BrowserReply::error(format!("{other:?}")),
                };
                let _ = reply.send(answer);
            }
        });
        let input = concat!(
            r#"{"token":"segredo","cmd":"info"}"#,
            "\n",
            r#"{"token":"errado","cmd":"info"}"#,
            "\n",
            "nao e json\n"
        );
        let mut conn = FakeStream {
            input: std::io::Cursor::new(input.as_bytes().to_vec()),
            output: Vec::new(),
        };
        serve_connection(&mut conn, "segredo", &tx);
        drop(tx);
        window.join().unwrap();
        let replies: Vec<BrowserReply> = String::from_utf8(conn.output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0].url.as_deref(), Some("about:blank"));
        assert_eq!(replies[1].error.as_deref(), Some("invalid token"));
        assert!(!replies[2].ok);
    }

    struct FakeStream {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl std::io::Read for &mut FakeStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for &mut FakeStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
