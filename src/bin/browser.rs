// Janela GUI no Windows: sem console junto (release).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! `code-terminal-browser [url]`: navegador em janela própria, controlado
//! pelo terminal (`code-terminal-cli browse`, Ctrl+clique em links).

fn main() {
    code_terminal::utils::logging::init_logging();
    let url = std::env::args().nth(1).unwrap_or_default();
    if let Err(error) = code_terminal::browser::window_app::run(&url) {
        eprintln!("code-terminal-browser: {error}");
        std::process::exit(1);
    }
}
