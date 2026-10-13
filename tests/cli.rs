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
fn imagem_remota_exige_flag() {
    let out = mdv(&["-p", "-"], "![x](https://example.com/a.png)\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("use --remote"));
}

#[test]
fn imagem_remota_em_rede_privada_e_bloqueada() {
    let out = mdv(&["-p", "--remote", "-"], "![x](http://127.0.0.1:9/a.png)\n");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("[imagem: x —"), "{text}");
    assert!(!text.contains("use --remote"), "{text}");
}

#[cfg(unix)]
#[test]
fn arquivo_nao_regular_e_rejeitado() {
    let out = mdv(&["-p", "/dev/zero"], "");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("/dev/zero"), "stderr: {err}");
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn largura_invalida_e_rejeitada() {
    let out = mdv(&["-p", "-w", "abc", "-"], "x\n");
    assert_eq!(out.status.code(), Some(2));
    let err = stderr_of(&out);
    assert!(err.contains("abc") && err.contains("largura"), "stderr: {err}");
}

#[test]
fn largura_sem_valor_e_rejeitada() {
    let out = mdv(&["-p", "-w"], "x\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr_of(&out).contains("largura"), "stderr: {}", stderr_of(&out));
}

#[test]
fn opcao_desconhecida_e_rejeitada() {
    let out = mdv(&["--nao-existe", "-"], "x\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr_of(&out).contains("--nao-existe"), "stderr: {}", stderr_of(&out));
}

#[test]
fn dois_arquivos_sao_rejeitados() {
    let out = mdv(&["a.md", "b.md"], "");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn duplo_hifen_encerra_as_opcoes() {
    let out = mdv(&["-p", "--", "-arquivo-inexistente.md"], "");
    assert_eq!(out.status.code(), Some(1));
    let err = stderr_of(&out);
    assert!(err.contains("-arquivo-inexistente.md"), "stderr: {err}");
    assert!(!err.contains("desconhecida"), "stderr: {err}");
}

#[test]
fn erro_de_leitura_nao_usa_formato_debug() {
    let out = mdv(&["-p", "nao-existe.md"], "");
    assert_eq!(out.status.code(), Some(1));
    let err = stderr_of(&out);
    assert!(err.starts_with("mdv: "), "stderr: {err}");
    assert!(!err.contains("Error:") && !err.contains('"'), "stderr: {err}");
}

#[test]
fn ajuda_sai_com_sucesso() {
    let out = mdv(&["--help"], "");
    assert!(out.status.success());
}

#[test]
fn pipe_fechado_na_saida_nao_e_erro() {
    let input = "linha de texto\n\n".repeat(100_000);
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(["-p", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mdv deve iniciar");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("escrever stdin");
    // Como `mdv ... | head -0`: o leitor some antes de a saída (bem maior que o pipe) acabar.
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("mdv deve terminar");
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(out.stderr.is_empty(), "stderr: {}", stderr_of(&out));
}
