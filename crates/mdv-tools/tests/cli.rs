use std::io::Write;
use std::process::{Command, Output, Stdio};

fn tools(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdv-tools"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mdv-tools deve iniciar");
    let _ = child.stdin.take().expect("stdin").write_all(stdin.as_bytes());
    child.wait_with_output().expect("mdv-tools deve terminar")
}

const DOC_EXTRACAO: &str = "intro\n\n# A\n\n```rust\nfn a() {}\n```\n\n## B\n\n```py\nprint('b')\n```\n\n```rust\nfn b() {}\n```\n\n# C\n\n```\nsem lang\n```\n";

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn check_links_limpo_sai_com_zero() {
    let out = tools(&["check-links", "-"], "# A\n\n[ok](#a) [web](https://example.com)\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "nenhum problema encontrado\n");
}

#[test]
fn check_links_com_problema_imprime_e_sai_com_um() {
    let out = tools(&["check-links", "-"], "# A\n\n[x](#nada)\n\n```\nsem fim\n");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("<stdin>:3: âncora \"#nada\" não existe"), "{text}");
    assert!(text.contains("<stdin>:5: cerca de código aberta"), "{text}");
    assert!(text.ends_with("2 problemas\n"), "{text}");
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn check_links_resolve_caminhos_a_partir_da_pasta_do_arquivo() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "# B\n").unwrap();
    let doc = dir.path().join("a.md");
    std::fs::write(&doc, "[b](b.md#b) [c](c.md)\n").unwrap();
    let out = tools(&["check-links", doc.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(":1: arquivo \"c.md\" não encontrado") && !text.contains("b.md"), "{text}");
}

#[test]
fn extract_code_imprime_todos_os_blocos_sem_cor() {
    let out = tools(&["extract-code", "-"], DOC_EXTRACAO);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "fn a() {}\n\nprint('b')\n\nfn b() {}\n\nsem lang\n");
    assert!(!stdout(&out).contains('\x1b'));
}

#[test]
fn extract_code_filtra_por_linguagem_e_n() {
    let out = tools(&["extract-code", "--lang", "RUST", "-n", "2", "-"], DOC_EXTRACAO);
    assert_eq!(stdout(&out), "fn b() {}\n");
}

#[test]
fn extract_code_n_fora_do_intervalo_sai_com_1() {
    let out = tools(&["extract-code", "--lang", "rust", "-n", "9", "-"], DOC_EXTRACAO);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("fora do intervalo"));
    assert!(stdout(&out).is_empty());
}

#[test]
fn extract_code_sem_bloco_sai_com_1() {
    let out = tools(&["extract-code", "-"], "só texto\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("nenhum bloco de código"));
}

#[test]
fn section_imprime_o_markdown_cru_com_subsecoes() {
    let out = tools(&["section", "a", "-"], DOC_EXTRACAO);
    let s = stdout(&out);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.starts_with("# A\n") && s.contains("## B") && !s.contains("# C"), "{s}");
}

#[test]
fn section_aninhada_para_no_proximo_titulo_do_mesmo_nivel() {
    let out = tools(&["section", "B", "-"], DOC_EXTRACAO);
    assert!(stdout(&out).starts_with("## B\n") && !stdout(&out).contains("fn a()"));
}

#[test]
fn section_ambigua_lista_candidatos_e_sai_com_1() {
    let out = tools(&["section", "passo", "-"], "# Passo um\n\n# Passo dois\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Passo um") && stderr(&out).contains("Passo dois"));
}

#[test]
fn section_inexistente_sai_com_1() {
    let out = tools(&["section", "zzz", "-"], DOC_EXTRACAO);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("zzz"));
}

#[test]
fn section_com_extract_code_extrai_so_dentro_da_secao() {
    let out = tools(&["extract-code", "--section", "B", "-"], DOC_EXTRACAO);
    assert_eq!(stdout(&out), "print('b')\n\nfn b() {}\n");
}

#[test]
fn extract_code_de_bloco_indentado() {
    let out = tools(&["extract-code", "-"], "texto\n\n    recuado\n");
    assert_eq!(stdout(&out), "recuado\n");
}

#[test]
fn extract_code_nao_deixa_escape_do_documento_passar() {
    let out = tools(&["extract-code", "-"], "```\na\x1b]0;titulo\x07b\n```\n");
    assert!(out.status.success());
    assert!(!stdout(&out).contains('\x1b') && !stdout(&out).contains('\x07'));
}

#[test]
fn extract_de_arquivo_funciona_como_de_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.md");
    std::fs::write(&p, DOC_EXTRACAO).unwrap();
    let out = tools(&["extract-code", "--lang", "py", p.to_str().unwrap()], "");
    assert_eq!(stdout(&out), "print('b')\n");
}

#[test]
fn section_ambigua_nao_deixa_escape_do_titulo_chegar_ao_stderr() {
    let out = tools(&["section", "passo", "-"], "# Passo ]0;xum

# Passo dois
");
    assert_eq!(out.status.code(), Some(1));
    assert!(!stderr(&out).contains('') && !stderr(&out).contains(''), "{:?}", stderr(&out));
}

#[test]
fn arquivo_inexistente_sai_com_1_sem_panico() {
    let out = tools(&["check-links", "/nao/existe/zzz.md"], "");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("mdv-tools:") && !err.contains("panicked"), "{err}");
}

#[test]
fn pasta_nao_e_arquivo() {
    let dir = tempfile::tempdir().unwrap();
    let out = tools(&["section", "A", dir.path().to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("não é um arquivo regular"));
}

#[test]
fn utf8_invalido_na_entrada_sai_com_1() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdv-tools"))
        .args(["extract-code", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&[0xff, 0xfe, b'\n']).unwrap();
    assert_eq!(child.wait_with_output().unwrap().status.code(), Some(1));
}

#[test]
fn escape_do_documento_nao_chega_ao_stdout() {
    let out = tools(&["section", "A", "-"], "# A\n\ntexto \x1b[31mvermelho\n");
    assert!(out.status.success());
    assert!(!out.stdout.contains(&0x1b), "{:?}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn sem_argumentos_e_uso_errado() {
    let out = tools(&[], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("subcomando"));
}

#[test]
fn ajuda_lista_os_tres_subcomandos() {
    let out = tools(&["--help"], "");
    let text = String::from_utf8_lossy(&out.stdout);
    for item in ["extract-code", "section", "check-links", "--lang", "-n", "--section"] {
        assert!(text.contains(item), "a ajuda não menciona {item}:\n{text}");
    }
}

#[test]
fn versao_imprime_nome_e_numero() {
    let out = tools(&["-V"], "");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), concat!("mdv-tools ", env!("CARGO_PKG_VERSION")));
}

// Windows não aceita ESC em nome de arquivo.
#[cfg(unix)]
#[test]
fn nome_de_arquivo_com_escape_nao_chega_ao_stdout_nem_ao_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("z\x1b[31m.md");
    std::fs::write(&doc, "[x](#nada)\n").unwrap();
    let out = tools(&["check-links", doc.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stdout.contains(&0x1b), "{:?}", String::from_utf8_lossy(&out.stdout));
    let ruim = dir.path().join("nao\x1b[31mexiste.md");
    let out = tools(&["section", "A", ruim.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.contains(&0x1b), "{:?}", String::from_utf8_lossy(&out.stderr));
}
