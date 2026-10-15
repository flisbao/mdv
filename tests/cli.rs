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
    // O mdv pode sair sem ler stdin (erro de uso): `BrokenPipe` aqui é esperado.
    let _ = child.stdin.take().expect("stdin").write_all(stdin.as_bytes());
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

#[test]
fn toc_imprime_indice_de_titulos() {
    let out = mdv(&["--toc", "-"], "# A\n\ntexto\n\n## B\n");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "A\n  B\n");
}

#[test]
fn stats_imprime_total_e_secoes() {
    let out = mdv(&["--stats", "-"], "# A\n\num dois\n\n## B\n\n```\nx\n```\n");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let linhas: Vec<&str> = text.lines().collect();
    assert_eq!(linhas.len(), 3, "{text}");
    assert!(linhas[0].starts_with("Total: 4 palavras · 1 bloco de código"), "{text}");
    assert!(linhas[2].starts_with("  B "), "{text}");
}

#[test]
fn latest_mostra_o_md_mais_novo_da_pasta() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# antigo\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(dir.path().join("a.md"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
        .unwrap();
    std::fs::write(dir.path().join("b.md"), "# novo\n").unwrap();
    let out = mdv(&["-p", "--latest", dir.path().to_str().unwrap()], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("novo") && !text.contains("antigo"), "{text}");
}

#[test]
fn latest_em_pasta_sem_md_e_erro_claro() {
    let dir = tempfile::tempdir().unwrap();
    let out = mdv(&["-p", "--latest", dir.path().to_str().unwrap()], "");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains(".md"));
}

#[test]
fn latest_exige_uma_pasta() {
    let out = mdv(&["-p", "--latest"], "");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--latest"));
}

#[test]
fn ajuda_documenta_subcomandos_e_opcoes() {
    let out = mdv(&["--help"], "");
    let text = String::from_utf8_lossy(&out.stdout);
    for item in [
        "mdv json", "mdv csv|tsv", "--remote", "--mermaid", "--follow", "--latest", "--toc", "--stats", "--as",
        "--path", "--head", "--cols", "--no-header", "--delimiter", "MDV_MMDC",
    ] {
        assert!(text.contains(item), "a ajuda não menciona {item}:\n{text}");
    }
}

#[test]
fn json_subcomando_com_path_mostra_so_o_trecho() {
    let out = mdv(
        &["json", "-p", "-", "--path", ".a.b"],
        "{\"a\":{\"b\":[1,2,{\"c\":\"x\"}]},\"z\":null}",
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("lista · 3 itens") && text.contains("\"c\": \"x\""), "{text}");
    assert!(!text.contains("\"z\""), "{text}");
}

#[test]
fn json_por_extensao_do_arquivo() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("d.json");
    std::fs::write(&f, "{\"nome\":\"mdv\"}").unwrap();
    let out = mdv(&["-p", f.to_str().unwrap()], "");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("objeto · 1 chaves") && text.contains("\"nome\": \"mdv\""), "{text}");
}

#[test]
fn json_invalido_mostra_erro_na_saida() {
    let out = mdv(&["json", "-p", "-"], "{");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("JSON inválido"), "{text}");
}

#[test]
fn csv_subcomando_mostra_tabela_com_rodape() {
    let out = mdv(&["csv", "-p", "-"], "id,nome,nota\n1,Ana,9.5\n2,Bia,10\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Ana") && text.contains("2 de 2 linhas · 3 colunas"), "{text}");
}

#[test]
fn tsv_com_cols_mostra_so_a_coluna() {
    let out = mdv(&["tsv", "-p", "--cols", "b", "-"], "a\tb\n1\t2\n");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("│ 2 │") && !text.contains("│ 1") && !text.contains("│ a"), "{text}");
}

#[test]
fn csv_com_head_e_delimiter() {
    let out = mdv(&["csv", "-p", "--delimiter", ";", "--head", "1", "-"], "n;m\n1;x\n2;y\n");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("1 de 2 linhas") && !text.contains('y'), "{text}");
}

const MERMAID_MD: &str = "```mermaid\ngraph TD; A-->B\n```\n";

#[test]
fn mermaid_sem_flag_mostra_codigo() {
    let out = mdv(&["-p", "-"], MERMAID_MD);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("╭─ mermaid") && text.contains("A-->B"), "{text}");
}

#[test]
fn mermaid_sem_mmdc_mostra_codigo_e_erro() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(["-p", "--mermaid", "-"])
        .env("MDV_MMDC", "/nao/existe/mmdc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("mdv deve iniciar");
    child.stdin.take().expect("stdin").write_all(MERMAID_MD.as_bytes()).expect("escrever stdin");
    let out = child.wait_with_output().expect("mdv deve terminar");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("mermaid (erro:") && text.contains("A-->B"), "{text}");
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

#[test]
fn json_lista_de_objetos_vira_tabela_por_padrao() {
    let out = mdv(&["json", "-p", "-"], "[{\"id\":1,\"nome\":\"Ana\"},{\"id\":2,\"nome\":\"Bia\"}]");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("│ nome") && text.contains("2 de 2 itens"), "{text}");
}

#[test]
fn json_as_text_forca_a_vista_indentada() {
    let out = mdv(&["json", "-p", "--as", "text", "-"], "[{\"id\":1}]");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("lista · 1 itens") && text.contains("\"id\": 1"), "{text}");
}

#[test]
fn as_com_markdown_e_erro_de_uso() {
    let out = mdv(&["--as", "table", "-"], "# a");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--as"));
}

#[test]
fn csv_com_celula_longa_quebra_em_vez_de_truncar() {
    let frase = "uma descricao bem longa que precisa quebrar em varias linhas dentro da celula final";
    let out = mdv(&["csv", "-p", "-w", "40", "-"], &format!("id,texto\n1,{frase}\n"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains('…'), "{text}");
    assert!(text.contains("final"), "{text}");
    assert!(text.lines().all(|l| l.chars().count() <= 40), "{text}");
}

#[test]
fn markdown_com_celula_longa_quebra_em_plain() {
    let md = "| a | b |\n|---|---|\n| 1 | palavra1 palavra2 palavra3 palavra4 palavra5 palavra6 palavra7 |\n";
    let out = mdv(&["-p", "-w", "30", "-"], md);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("palavra7") && !text.contains('…'), "{text}");
}

#[test]
fn table_zero_e_erro_de_uso() {
    let out = mdv(&["--table", "0", "-"], "# x\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--table"));
}

#[test]
fn table_e_ignorado_em_plain() {
    let out = mdv(&["-p", "--table", "9", "-"], "| a |\n|---|\n| 1 |\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains('┌'));
}

#[test]
fn memory_buffer_invalido_e_erro_de_uso() {
    let out = mdv(&["--memory-buffer", "abc", "-"], "# x\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--memory-buffer"));
}

fn mdv_env(args: &[&str], stdin: &str, env: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mdv"));
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(v) = env {
        cmd.env("MDV_MEMORY_BUFFER", v);
    } else {
        cmd.env_remove("MDV_MEMORY_BUFFER");
    }
    let mut child = cmd.spawn().expect("mdv deve iniciar");
    let _ = child.stdin.take().expect("stdin").write_all(stdin.as_bytes());
    child.wait_with_output().expect("mdv deve terminar")
}

#[test]
fn variavel_de_ambiente_invalida_e_erro_de_uso_que_cita_a_variavel() {
    let out = mdv_env(&["-p", "-"], "# x\n", Some("muito"));
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("MDV_MEMORY_BUFFER"));
}

#[test]
fn flag_valida_vence_variavel_invalida() {
    let out = mdv_env(&["-p", "--memory-buffer", "256M", "-"], "# x\n", Some("muito"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn limite_maior_que_a_memoria_livre_avisa_no_stderr_e_segue() {
    if !std::path::Path::new("/proc/meminfo").exists() {
        return;
    }
    let out = mdv(&["-p", "--memory-buffer", "999999G", "-"], "# x\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("aviso: o limite de memória"));
    assert!(String::from_utf8_lossy(&out.stdout).contains('x'));
}

#[test]
fn markdown_que_expande_alem_do_limite_para_com_mensagem() {
    // 4 MiB de parágrafos de uma letra: ~1,4 milhão de parágrafos, centenas de MiB renderizados.
    let md = "a\n\n".repeat(4 * 1024 * 1024 / 3);
    let out = mdv(&["-p", "--memory-buffer", "32M", "-"], &md);
    assert!(out.status.success(), "não pode abortar: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("limite de memória de 32 MiB atingido"), "{}", &text[..text.len().min(300)]);
    assert!(text.contains("--memory-buffer"), "{}", &text[..text.len().min(300)]);
    assert!(text.lines().count() < 5, "o documento parcial é descartado");
}

#[test]
fn arquivo_grande_demais_para_o_limite_e_recusado_antes_de_ler() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("grande.md");
    std::fs::write(&p, "x".repeat(9 * 1024 * 1024)).unwrap();
    let out = mdv(&["-p", "--memory-buffer", "32M", p.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("limite de memória de 32 MiB"));
}

#[test]
fn json_de_numeros_pequenos_que_expande_para_com_mensagem() {
    // 3 MiB de `0,` viram ~48 MiB de `Value`: tem que parar antes de montar a árvore.
    let json = format!("[{}0]", "0,".repeat(3 * 1024 * 1024 / 2));
    let out = mdv(&["json", "-p", "--memory-buffer", "32M", "-"], &json);
    assert!(out.status.success(), "não pode abortar: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("limite de memória de 32 MiB atingido"));
}

#[test]
fn json_com_chave_gigante_respeita_o_limite_padrao() {
    // ~1 MiB de chave com 100 mil filhos: o flatten antigo copiava a chave para cada filho.
    let filhos: String = (0..100_000).map(|i| format!("\"k{i}\":0")).collect::<Vec<_>>().join(",");
    let json = format!("[{{\"{}\":{{{filhos}}}}}]", "A".repeat(1_000_000));
    let out = mdv(&["json", "-p", "-"], &json);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

const DOC_EXTRACAO: &str = "intro\n\n# A\n\n```rust\nfn a() {}\n```\n\n## B\n\n```py\nprint('b')\n```\n\n```rust\nfn b() {}\n```\n\n# C\n\n```\nsem lang\n```\n";

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn extract_code_imprime_todos_os_blocos_sem_cor() {
    let out = mdv(&["--extract-code", "-"], DOC_EXTRACAO);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "fn a() {}\n\nprint('b')\n\nfn b() {}\n\nsem lang\n");
    assert!(!stdout(&out).contains('\x1b'));
}

#[test]
fn extract_code_filtra_por_linguagem_e_n() {
    let out = mdv(&["--extract-code", "--lang", "RUST", "-n", "2", "-"], DOC_EXTRACAO);
    assert_eq!(stdout(&out), "fn b() {}\n");
}

#[test]
fn extract_code_n_fora_do_intervalo_sai_com_1() {
    let out = mdv(&["--extract-code", "--lang", "rust", "-n", "9", "-"], DOC_EXTRACAO);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("fora do intervalo"));
    assert!(stdout(&out).is_empty());
}

#[test]
fn extract_code_sem_bloco_sai_com_1() {
    let out = mdv(&["--extract-code", "-"], "só texto\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("nenhum bloco de código"));
}

#[test]
fn section_imprime_o_markdown_cru_com_subsecoes() {
    let out = mdv(&["--section", "a", "-"], DOC_EXTRACAO);
    let s = stdout(&out);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.starts_with("# A\n") && s.contains("## B") && !s.contains("# C"), "{s}");
}

#[test]
fn section_aninhada_para_no_proximo_titulo_do_mesmo_nivel() {
    let out = mdv(&["--section", "B", "-"], DOC_EXTRACAO);
    assert!(stdout(&out).starts_with("## B\n") && !stdout(&out).contains("fn a()"));
}

#[test]
fn section_ambigua_lista_candidatos_e_sai_com_1() {
    let out = mdv(&["--section", "passo", "-"], "# Passo um\n\n# Passo dois\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Passo um") && stderr(&out).contains("Passo dois"));
}

#[test]
fn section_inexistente_sai_com_1() {
    let out = mdv(&["--section", "zzz", "-"], DOC_EXTRACAO);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("zzz"));
}

#[test]
fn section_com_extract_code_extrai_so_dentro_da_secao() {
    let out = mdv(&["--section", "B", "--extract-code", "-"], DOC_EXTRACAO);
    assert_eq!(stdout(&out), "print('b')\n\nfn b() {}\n");
}

#[test]
fn extract_code_de_bloco_indentado() {
    let out = mdv(&["--extract-code", "-"], "texto\n\n    recuado\n");
    assert_eq!(stdout(&out), "recuado\n");
}

#[test]
fn extract_code_nao_deixa_escape_do_documento_passar() {
    let out = mdv(&["--extract-code", "-"], "```\na\x1b]0;titulo\x07b\n```\n");
    assert!(out.status.success());
    assert!(!stdout(&out).contains('\x1b') && !stdout(&out).contains('\x07'));
}

#[test]
fn extract_de_arquivo_funciona_como_de_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.md");
    std::fs::write(&p, DOC_EXTRACAO).unwrap();
    let out = mdv(&["--extract-code", "--lang", "py", p.to_str().unwrap()], "");
    assert_eq!(stdout(&out), "print('b')\n");
}

#[test]
fn flags_de_extracao_invalidas_sao_erro_de_uso() {
    for args in [
        &["--lang", "rust", "-"][..],
        &["--extract-code", "-n", "0", "-"][..],
        &["json", "--section", "A", "-"][..],
        &["--extract-code", "--toc", "-"][..],
    ] {
        let out = mdv(args, "# A\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
    }
}

#[test]
fn section_ambigua_nao_deixa_escape_do_titulo_chegar_ao_stderr() {
    let out = mdv(&["--section", "passo", "-"], "# Passo ]0;xum

# Passo dois
");
    assert_eq!(out.status.code(), Some(1));
    assert!(!stderr(&out).contains('') && !stderr(&out).contains(''), "{:?}", stderr(&out));
}

#[test]
fn graphics_kitty_no_modo_plain_continua_em_meio_bloco() {
    let dir = tempfile::tempdir().unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 10, 10, 255]))
        .save(dir.path().join("a.png"))
        .unwrap();
    let md = dir.path().join("a.md");
    std::fs::write(&md, "![x](a.png)\n").unwrap();
    let out = mdv(&["-p", "--graphics", "kitty", md.to_str().unwrap()], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains('▀'), "{text:?}");
    assert!(!text.contains('\u{10EEEE}') && !text.contains("\x1b_G"), "{text:?}");
}

#[test]
fn graphics_invalido_e_erro_de_uso() {
    let out = mdv(&["--graphics", "sixel", "-"], "");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--graphics"));
}
