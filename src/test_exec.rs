//! Executáveis falsos (scripts `sh`) para os testes de unidade.
//!
//! Gravar um executável e rodá-lo logo em seguida corre o risco de
//! `ETXTBSY` ("Text file busy"): se outro teste fizer `fork` enquanto o
//! arquivo está aberto para escrita, o filho herda o descritor até dar
//! `exec`, e o kernel recusa executar um arquivo aberto para escrita. Com
//! os testes rodando em paralelo (e vários deles abrindo processos), isso
//! aparecia de vez em quando no CI.
//!
//! [`write_fake_executable`] grava num temporário, faz `fsync`, troca pelo
//! nome final com `rename` (atômico: ninguém executa um arquivo pela metade)
//! e só devolve quando o script já roda (repete o `exec` em `ETXTBSY`).

use std::path::Path;
use std::time::Duration;

/// Argumento que o script responde saindo na hora (usado para esperar o
/// arquivo ficar executável sem rodar o corpo dele).
pub const PROBE_ARG: &str = "__probe";

/// Vezes que o `exec` é repetido em `ETXTBSY` (com 5 ms entre elas).
const BUSY_RETRIES: usize = 400;

/// Cria `path` como script `sh` que roda `body`. Devolve o caminho.
pub fn write_fake_executable(path: &Path, body: &str) -> String {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let script = format!("#!/bin/sh\n[ \"$1\" = {PROBE_ARG} ] && exit 0\n{body}\n");
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .open(&tmp)
            .unwrap();
        file.write_all(script.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&tmp, path).unwrap();
    wait_until_executable(path);
    path.to_string_lossy().to_string()
}

/// Espera o script poder ser executado: um filho de outro teste pode estar
/// segurando o descritor de escrita até dar `exec`.
pub fn wait_until_executable(path: &Path) {
    for _ in 0..BUSY_RETRIES {
        match std::process::Command::new(path).arg(PROBE_ARG).status() {
            Err(e) if e.raw_os_error() == Some(nix::libc::ETXTBSY) => {
                std::thread::sleep(Duration::from_millis(5))
            }
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vários scripts gravados e rodados ao mesmo tempo, com outras threads
    /// fazendo fork: nenhum falha com "Text file busy". Só no Linux: é lá que
    /// o ETXTBSY acontece (e no macOS esta rajada de processos derrubou o
    /// binário de testes com SIGABRT).
    #[cfg(target_os = "linux")]
    #[test]
    fn test_fake_executables_run_under_parallel_forks() {
        let dir = tempfile::tempdir().unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let forkers: Vec<_> = (0..4)
            .map(|_| {
                let stop = std::sync::Arc::clone(&stop);
                std::thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = std::process::Command::new("true").status();
                    }
                })
            })
            .collect();
        let workers: Vec<_> = (0..8)
            .map(|i| {
                let path = dir.path().join(format!("fake{i}"));
                std::thread::spawn(move || {
                    for round in 0..20 {
                        let program = write_fake_executable(&path, &format!("echo r{round}"));
                        let out = std::process::Command::new(&program).output().unwrap();
                        assert_eq!(
                            String::from_utf8_lossy(&out.stdout).trim(),
                            format!("r{round}")
                        );
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for forker in forkers {
            forker.join().unwrap();
        }
    }

    #[test]
    fn test_probe_does_not_run_the_body() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("rodou");
        let program = write_fake_executable(
            &dir.path().join("fake"),
            &format!("touch '{}'", marker.display()),
        );
        assert!(!marker.exists(), "the probe ran the body");
        std::process::Command::new(&program).status().unwrap();
        assert!(marker.exists());
    }
}
