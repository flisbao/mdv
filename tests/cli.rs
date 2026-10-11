use std::io::Write;
use std::process::{Command, Output, Stdio};

fn mdv(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mdv deve iniciar");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("escrever stdin");
    child.wait_with_output().expect("mdv deve terminar")
}

#[test]
fn largura_gigante_e_limitada() {
    let out = mdv(&["-p", "-w", "18446744073709551615", "-"], "---\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let widest = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    assert!(widest <= 1000, "linha com {widest} colunas");
}

#[test]
fn arquivo_nao_regular_e_rejeitado() {
    let out = mdv(&["-p", "/dev/zero"], "");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("/dev/zero"), "stderr: {err}");
}
