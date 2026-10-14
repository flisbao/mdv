//! Renderiza Mermaid chamando o `mmdc` (mermaid-cli) como subprocesso.

use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub const MAX_MERMAID_BYTES: usize = 64 * 1024;
pub const MMDC_TIMEOUT: Duration = Duration::from_secs(20);

pub fn args(input: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "-i".into(),
        input.into(),
        "-o".into(),
        output.into(),
        "-b".into(),
        "transparent".into(),
        "-w".into(),
        "1200".into(),
        "-q".into(),
    ]
}

/// Quantos `mmdc` (cada um abre um Chromium) podem rodar ao mesmo tempo.
pub const MAX_CONCURRENT_MMDC: usize = 2;

static RUNNING: Mutex<usize> = Mutex::new(0);
static SLOT_FREED: Condvar = Condvar::new();

/// Executa `f` quando houver uma vaga; o resto espera na fila.
pub fn with_slot<T>(f: impl FnOnce() -> T) -> T {
    let mut running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    while *running >= MAX_CONCURRENT_MMDC {
        running = SLOT_FREED.wait(running).unwrap_or_else(PoisonError::into_inner);
    }
    *running += 1;
    drop(running);
    let out = f();
    *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
    SLOT_FREED.notify_one();
    out
}

/// Inicia o `mmdc`. `ETXTBSY` (executável ainda aberto para escrita por outro processo, comum
/// logo após instalar/gerar o binário) é transitório: tenta de novo por um instante.
fn spawn_mmdc(bin: &str, input: &Path, output: &Path) -> std::io::Result<std::process::Child> {
    let mut attempts = 0;
    loop {
        let res = Command::new(bin)
            .args(args(input, output))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        match res {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 20 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(25));
            }
            other => return other,
        }
    }
}

pub fn render_png(source: &str, bin: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    if source.len() > MAX_MERMAID_BYTES {
        return Err("diagrama grande demais".into());
    }
    with_slot(|| run_mmdc(source, bin, timeout))
}

fn run_mmdc(source: &str, bin: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (input, output) = (dir.path().join("in.mmd"), dir.path().join("out.png"));
    std::fs::write(&input, source).map_err(|e| e.to_string())?;

    let mut child = spawn_mmdc(bin, &input, &output)
        .map_err(|e| format!("não consegui executar mmdc ({bin}): {e}"))?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("tempo esgotado no mmdc".into());
            }
            None => std::thread::sleep(Duration::from_millis(30)),
        }
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(e) = child.stderr.take() {
            let _ = e.take(2048).read_to_string(&mut err);
        }
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("falha desconhecida");
        return Err(format!("mmdc: {}", first.trim()));
    }
    std::fs::read(&output).map_err(|e| format!("mmdc não gerou a imagem: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn argumentos_do_mmdc() {
        let a = args(Path::new("/t/in.mmd"), Path::new("/t/out.png"));
        let a: Vec<String> = a.iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(a, ["-i", "/t/in.mmd", "-o", "/t/out.png", "-b", "transparent", "-w", "1200", "-q"]);
    }

    #[test]
    fn no_maximo_n_mmdc_ao_mesmo_tempo() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let (cur, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (cur, peak) = (Arc::clone(&cur), Arc::clone(&peak));
                std::thread::spawn(move || {
                    with_slot(|| {
                        let now = cur.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(40));
                        cur.fetch_sub(1, Ordering::SeqCst);
                    });
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let peak = peak.load(Ordering::SeqCst);
        assert!((1..=MAX_CONCURRENT_MMDC).contains(&peak), "pico de {peak}");
    }

    #[test]
    fn binario_inexistente_da_erro_claro() {
        let e = render_png("graph TD; A-->B", "/nao/existe/mmdc", Duration::from_secs(2)).unwrap_err();
        assert!(e.contains("mmdc"), "{e}");
    }

    #[test]
    fn fonte_grande_demais_e_recusada() {
        let src = "A".repeat(MAX_MERMAID_BYTES + 1);
        assert!(render_png(&src, "mmdc", Duration::from_secs(2)).unwrap_err().contains("grande"));
    }

    #[cfg(unix)]
    fn fake(dir: &Path, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("fake-mmdc");
        std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    #[test]
    fn devolve_png_gerado_pelo_binario() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("fixture.png");
        std::fs::write(&png, b"PNGDATA").unwrap();
        // $4 é o caminho de saída (-i in -o out ...)
        let bin = fake(dir.path(), &format!("cp {} \"$4\"", png.display()));
        assert_eq!(render_png("graph TD; A-->B", &bin, Duration::from_secs(5)).unwrap(), b"PNGDATA");
    }

    #[cfg(unix)]
    #[test]
    fn estoura_timeout_e_mata_o_processo() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "sleep 10");
        let t = Instant::now();
        let e = render_png("graph TD; A-->B", &bin, Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("tempo"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn saida_de_erro_do_binario_vira_mensagem() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "echo 'Parse error on line 1' >&2; exit 1");
        let e = render_png("x", &bin, Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("Parse error"), "{e}");
    }
}
