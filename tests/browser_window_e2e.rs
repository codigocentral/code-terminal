//! Navegador em janela própria de ponta a ponta: o binário real abre uma
//! página local e é controlado pelo socket (texto, JavaScript, clique,
//! voltar, fechar).
//!
//! Abre uma janela na tela, então só roda com `CODE_TERMINAL_BROWSER_E2E=1`
//! (precisa de Wayland ou X11; no CI não há tela).

use code_terminal::browser::remote::{request_on, BrowserReply, BrowserRequest, BrowserStream};
use std::time::{Duration, Instant};

fn enabled() -> bool {
    std::env::var("CODE_TERMINAL_BROWSER_E2E").as_deref() == Ok("1")
}

fn ask(request: BrowserRequest) -> std::io::Result<BrowserReply> {
    let (mut stream, token): (Box<dyn BrowserStream>, String) =
        code_terminal::browser::remote::connect_default()?;
    request_on(stream.as_mut(), &token, request)
}

fn wait_until(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn test_browser_window_is_driven_through_its_socket() {
    if !enabled() {
        eprintln!("skipped: set CODE_TERMINAL_BROWSER_E2E=1 (opens a window)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let index = dir.path().join("index.html");
    std::fs::write(
        &index,
        r#"<!doctype html><title>Inicio</title><p>2 + 2 = <span id="r"></span></p>
<script>document.getElementById('r').textContent = 2 + 2;</script>
<a id="link" href="segunda.html">segunda</a>"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("segunda.html"),
        "<!doctype html><title>Segunda</title><h1>Segunda página</h1>",
    )
    .unwrap();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_code-terminal-browser"))
        .arg(&index)
        .spawn()
        .unwrap();
    assert!(
        wait_until(|| ask(BrowserRequest::Info).is_ok_and(|r| r.title.as_deref() == Some("Inicio"))),
        "browser did not load the page"
    );

    let text = ask(BrowserRequest::Text)
        .unwrap()
        .result
        .unwrap_or_default();
    assert!(text.contains("2 + 2 = 4"), "{text}");
    let links = ask(BrowserRequest::Eval {
        js: "document.querySelectorAll('a').length".into(),
    })
    .unwrap();
    assert_eq!(links.result.as_deref(), Some("1"));

    ask(BrowserRequest::Eval {
        js: "document.getElementById('link').click()".into(),
    })
    .unwrap();
    assert!(wait_until(
        || ask(BrowserRequest::Info).is_ok_and(|r| r.title.as_deref() == Some("Segunda"))
    ));
    ask(BrowserRequest::Back).unwrap();
    assert!(wait_until(
        || ask(BrowserRequest::Info).is_ok_and(|r| r.title.as_deref() == Some("Inicio"))
    ));

    // `javascript:` não passa.
    let refused = ask(BrowserRequest::Open {
        url: "javascript:alert(1)".into(),
    })
    .unwrap();
    assert!(!refused.ok);

    assert!(ask(BrowserRequest::Close).unwrap().ok);
    assert!(wait_until(|| child.try_wait().is_ok_and(|s| s.is_some())));
    #[cfg(unix)]
    assert!(!code_terminal::browser::remote::default_browser_socket().exists());
}
