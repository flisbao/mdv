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
fn flags_que_foram_para_o_mdv_tools_sao_rejeitadas() {
    for args in [
        vec!["--check-links", "-"],
        vec!["--extract-code", "-"],
        vec!["--lang", "rust", "-"],
        vec!["-n", "1", "-"],
    ] {
        let out = mdv(&args, "# A\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(&format!("opção desconhecida: {}", args[0])), "{args:?}: {err}");
    }
}

#[test]
fn ajuda_do_mdv_nao_menciona_as_flags_movidas() {
    let out = mdv(&["--help"], "");
    let text = String::from_utf8_lossy(&out.stdout);
    for item in ["--check-links", "--extract-code", "--lang"] {
        assert!(!text.contains(item), "a ajuda ainda menciona {item}");
    }
}

#[test]
fn latest_foi_removido() {
    let out = mdv(&["-p", "--latest", "."], "");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("opção desconhecida: --latest"), "{err}");
}

#[test]
fn ajuda_documenta_subcomandos_e_opcoes() {
    let out = mdv(&["--help"], "");
    let text = String::from_utf8_lossy(&out.stdout);
    for item in [
        "mdv json", "mdv yaml", "mdv csv|tsv", "--remote", "--follow", "--toc", "--stats", "--as",
        "--path", "--head", "--cols", "--no-header", "--delimiter",
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
    assert!(text.contains("✗ linha 1, coluna 1") && text.contains("fim inesperado"), "{text}");
}

#[test]
fn yaml_valido_em_modo_plain_mostra_o_texto() {
    // Com stdout em pipe o `--plain` não usa cores; a coloração do YAML é coberta em `doc.rs`.
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("d.yaml");
    std::fs::write(&f, "nome: mdv\nlista:\n  - 1\n").unwrap();
    let out = mdv(&["-p", f.to_str().unwrap()], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("nome: mdv") && text.contains("- 1"), "{text}");
}

#[test]
fn yaml_invalido_mostra_a_linha_do_erro() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("d.yaml");
    std::fs::write(&f, "a:\n  b: 1\n c: 2\n").unwrap();
    let out = mdv(&["-p", f.to_str().unwrap()], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("✗ linha 3"), "{text}");
}

#[test]
fn yaml_via_stdin_com_subcomando() {
    let out = mdv(&["yaml", "-p", "-"], "a:\n  b: [1, 2]\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("b: [1, 2]"), "{text}");
}

fn sample(nome: &str) -> String {
    format!("{}/samples/{nome}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn exemplo_yaml_quebrado_mostra_a_linha_do_erro() {
    let out = mdv(&["-p", &sample("quebrado.yaml")], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("✗ linha 5"), "{text}");
}

#[test]
fn exemplo_json_quebrado_mostra_a_linha_do_erro() {
    let out = mdv(&["-p", &sample("quebrado.json")], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("✗ linha 7"), "{text}");
}

#[test]
fn exemplos_yaml_validos_saem_sem_erro() {
    for nome in ["k8s.yaml", "config.yaml", "github-actions.yml", "cloudformation.yaml", "ancoras.yaml"] {
        let out = mdv(&["-p", &sample(nome)], "");
        assert!(out.status.success(), "{nome}: {}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains('✗'), "{nome}: {text}");
    }
}

/// Roda o `mdv` sob `ulimit -v` (o processo morre sozinho se passar do teto, sem travar a máquina).
#[cfg(target_os = "linux")]
fn mdv_com_teto(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new("sh")
        .args(["-c", "ulimit -v 2000000 && exec \"$0\" \"$@\"", env!("CARGO_BIN_EXE_mdv")])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh deve iniciar");
    let _ = child.stdin.take().expect("stdin").write_all(stdin.as_bytes());
    child.wait_with_output().expect("mdv deve terminar")
}

/// Bomba de aliases (billion laughs): `l0` tem 10 itens e cada nível seguinte lista 10 vezes o
/// anterior, então `l9` teria 10^10 nós. A entrada tem poucas centenas de bytes.
#[cfg(target_os = "linux")]
fn bomba_de_aliases() -> String {
    let mut t = format!("l0: &l0 [{}]\n", ["x"; 10].join(", "));
    for i in 1..=9 {
        let alias = format!("*l{}", i - 1);
        t.push_str(&format!("l{i}: &l{i} [{}]\n", vec![alias; 10].join(", ")));
    }
    t
}

#[cfg(target_os = "linux")] // `ulimit -v` não é confiável no macOS nem existe no Windows
#[test]
fn yaml_com_aliases_em_excesso_nao_estoura() {
    let bomba = bomba_de_aliases();
    assert!(bomba.len() <= 64 << 10, "a entrada passa de 64 KiB");
    // Vista de tabela e de esquema: o teto de cópias (1 milhão de nós clonados por alias) barra a
    // conversão. Pior caso: ~1 milhão de nós vivos (~100 B cada, ~100 MB; medido: pico de 98 MB),
    // longe do `ulimit -v` de ~2 GB e do limite padrão de 512 MiB.
    for vista in ["table", "schema"] {
        let out = mdv_com_teto(&["yaml", "-p", "-", "--as", vista], &bomba);
        let (so, se) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(!se.contains("panicked") && !so.contains("panicked"), "{vista}: {se}");
        assert!(out.status.success(), "{vista}: {se}");
        assert!(so.contains("estrutura grande demais"), "{vista}: {so}");
    }
    // Vista de texto: mostra o original, sem expandir os aliases.
    let out = mdv_com_teto(&["yaml", "-p", "-", "--as", "text"], &bomba);
    let (so, se) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && !se.contains("panicked"), "{se}");
    assert!(so.contains("l9: &l9 [*l8, *l8"), "{so}");
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
const MERMAID_INVALIDO_MD: &str = "```mermaid\nisto não é mermaid\n```\n";

#[test]
fn mermaid_e_setup_mermaid_foram_removidos() {
    for flag in ["--mermaid", "--setup-mermaid"] {
        let out = mdv(&["-p", flag, "-"], "# a\n");
        assert_eq!(out.status.code(), Some(2), "{flag}");
        assert!(stderr(&out).contains(&format!("opção desconhecida: {flag}")), "{}", stderr(&out));
    }
}

#[test]
fn mermaid_em_plain_sem_graficos_sai_como_codigo() {
    for md in [MERMAID_MD, MERMAID_INVALIDO_MD] {
        let out = mdv(&["-p", "-"], md);
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success());
        assert!(text.contains("╭─ mermaid") && !text.contains("mermaid ("), "{text}");
        assert!(!text.contains('▀') && !text.contains("erro:"), "{text}");
        assert!(!text.contains("--mermaid") && !text.contains("setup-mermaid") && !text.contains("mmdc"), "{text}");
    }
    let text = String::from_utf8_lossy(&mdv(&["-p", "-"], MERMAID_MD).stdout).into_owned();
    assert!(text.contains("graph TD"), "{text}");
}

/// Sem a feature `mermaid` o diagrama é só um bloco de código, sem erro de renderização.
#[cfg(not(feature = "mermaid"))]
#[test]
fn sem_a_feature_mermaid_o_diagrama_sai_como_codigo() {
    let out = mdv(&["-p", "-"], MERMAID_MD);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(text.contains("graph TD") && text.contains("mermaid"), "{text}");
    assert!(!text.contains("erro:") && !text.contains("renderizando") && !text.contains('▀'), "{text}");
}

#[cfg(not(feature = "remote"))]
#[test]
fn sem_a_feature_remote_a_flag_e_aceita_e_a_imagem_mostra_o_erro_claro() {
    // `--remote` continua válida; a imagem remota falha com a mensagem clara e o resto do documento sai.
    let out = mdv(&["-p", "--remote", "-"], "antes\n\n![x](http://127.0.0.1:9/a.png)\n\ndepois\n");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(text.contains("antes") && text.contains("depois"), "{text}");
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" "); // a mensagem pode quebrar de linha
    assert!(flat.contains("recompile com --features remote"), "{text}");
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
fn stats_de_markdown_que_expande_alem_do_limite_da_erro_limpo() {
    let md = "a\n\n".repeat(4 * 1024 * 1024 / 3);
    let out = mdv(&["--stats", "--memory-buffer", "32M", "-"], &md);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("limite de memória de 32 MiB atingido"), "{}", &err[..err.len().min(300)]);
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

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
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

#[test]
fn chat_foi_removido() {
    // `chat` deixou de ser subcomando: vira o primeiro caminho e o `-` é o segundo.
    let out = mdv(&["chat", "-p", "-"], "{\"role\":\"user\",\"content\":\"oi\"}\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("informe um único arquivo"), "{}", stderr(&out));
}

#[test]
fn diff_e_split_foram_removidos() {
    let out = mdv(&["--split", "-p", "-"], "# a\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("opção desconhecida: --split"), "{}", stderr(&out));

    // `diff` deixou de ser subcomando: vira nome de arquivo (que não existe).
    let out = mdv(&["-p", "diff"], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("não consegui ler diff"), "{}", stderr(&out));
}

fn tmp_out(name: &str) -> (tempfile::TempDir, String) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join(name).to_str().unwrap().to_string();
    (d, p)
}

#[test]
fn exporta_stdin_para_pdf_docx_odt() {
    for (name, magic) in [("a.pdf", &b"%PDF"[..]), ("a.docx", b"PK"), ("a.odt", b"PK")] {
        let (_d, out) = tmp_out(name);
        let res = mdv(&["-o", &out], "# Oi\n\ntexto **forte**\n");
        assert!(res.status.success(), "{name}: {}", String::from_utf8_lossy(&res.stderr));
        assert!(String::from_utf8_lossy(&res.stdout).contains("exportado:"), "{name}");
        assert!(std::fs::read(&out).unwrap().starts_with(magic), "{name}");
    }
}

#[test]
fn exporta_csv_e_tsv_para_xlsx_e_ods() {
    for (fmt, stdin) in [("csv", "a,b\n1,2\n"), ("tsv", "a\tb\n1\t2\n")] {
        for name in ["t.xlsx", "t.ods"] {
            let (_d, out) = tmp_out(name);
            let res = mdv(&[fmt, "-", "-o", &out], stdin);
            assert!(res.status.success(), "{fmt} {name}: {}", String::from_utf8_lossy(&res.stderr));
            assert!(std::fs::read(&out).unwrap().starts_with(b"PK"), "{fmt} {name}");
        }
    }
}

#[test]
fn extrai_tabela_do_markdown_para_planilha_por_secao_e_por_numero() {
    let md = "# A\n\n| x |\n|---|\n| 1 |\n\n# B\n\n| y |\n|---|\n| 2 |\n";
    for args in [&["--section", "B"][..], &["--table", "2"][..], &[][..]] {
        let (_d, out) = tmp_out("t.xlsx");
        let mut a = vec!["-o", out.as_str()];
        a.extend_from_slice(args);
        let res = mdv(&a, md);
        assert!(res.status.success(), "{args:?}: {}", String::from_utf8_lossy(&res.stderr));
        assert!(std::fs::read(&out).unwrap().starts_with(b"PK"), "{args:?}");
    }
}

#[test]
fn markdown_sem_tabela_para_planilha_sai_com_1_e_nao_cria_arquivo() {
    let (_d, out) = tmp_out("t.ods");
    let res = mdv(&["-o", &out], "# so texto\n");
    assert_eq!(res.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&res.stderr).contains("não tem tabelas"));
    assert!(!std::path::Path::new(&out).exists());
}

#[test]
fn csv_para_pdf_sai_com_1_e_nao_cria_arquivo() {
    let (_d, out) = tmp_out("t.pdf");
    let res = mdv(&["csv", "-", "-o", &out], "a\n1\n");
    assert_eq!(res.status.code(), Some(1));
    assert!(!std::path::Path::new(&out).exists());
}

#[test]
fn exportar_recusa_sobrescrever_sem_force() {
    let (_d, out) = tmp_out("a.xlsx");
    std::fs::write(&out, "antigo").unwrap();
    let res = mdv(&["csv", "-", "-o", &out], "a\n1\n");
    assert_eq!(res.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&res.stderr).contains("--force"));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "antigo");
    let res = mdv(&["csv", "-", "-o", &out, "--force"], "a\n1\n");
    assert!(res.status.success());
    assert!(std::fs::read(&out).unwrap().starts_with(b"PK"));
}

#[test]
fn exportar_formato_desconhecido_sai_com_1_e_lista_os_formatos() {
    let (_d, out) = tmp_out("a.txt");
    let res = mdv(&["-o", &out], "x\n");
    assert_eq!(res.status.code(), Some(1));
    let err = String::from_utf8_lossy(&res.stderr);
    assert!(err.contains(".xlsx") && err.contains(".pdf"), "{err}");
    assert!(!std::path::Path::new(&out).exists());
}

#[test]
fn exportar_combinacoes_invalidas_saem_com_2() {
    for args in [
        vec!["--toc", "-o", "a.pdf"],
        vec!["--force"],
        vec!["--section", "A"],
        vec!["-o", "a.xlsx", "--section", "A", "--table", "1"],
        vec!["csv", "-", "-o", "a.xlsx", "--head", "3"],
        vec!["json", "-", "-o", "a.xlsx"],
    ] {
        let res = mdv(&args, "# t\n");
        assert_eq!(res.status.code(), Some(2), "{args:?}");
        assert!(!String::from_utf8_lossy(&res.stderr).is_empty(), "{args:?}");
    }
}

#[test]
fn exporta_as_amostras_do_repositorio() {
    for (sample, name) in [
        ("samples/exportar.md", "x.pdf"),
        ("samples/exportar.md", "x.docx"),
        ("samples/exportar.md", "x.odt"),
        ("samples/tabelas-export.md", "x.xlsx"),
        ("samples/tabelas-export.md", "x.ods"),
        ("samples/dados.csv", "x.xlsx"),
        ("samples/dados.tsv", "x.ods"),
    ] {
        let (_d, out) = tmp_out(name);
        let res = Command::new(env!("CARGO_BIN_EXE_mdv")).args([sample, "-o", &out]).output().unwrap();
        assert!(res.status.success(), "{sample} → {name}: {}", String::from_utf8_lossy(&res.stderr));
    }
}

#[test]
fn exportar_com_limite_acima_da_memoria_livre_avisa_uma_vez_no_stderr() {
    if !std::path::Path::new("/proc/meminfo").exists() {
        return;
    }
    let (_d, out) = tmp_out("a.pdf");
    let res = mdv(&["--memory-buffer", "999999G", "-o", &out], "# x\n");
    assert!(res.status.success(), "{}", String::from_utf8_lossy(&res.stderr));
    assert_eq!(String::from_utf8_lossy(&res.stderr).matches("aviso: o limite de memória").count(), 1);
}

/// Grava um `.msg` de teste numa pasta temporária e devolve o caminho.
fn msg_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> String {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).unwrap();
    p.to_string_lossy().into_owned()
}

fn email_com_tabela() -> Vec<u8> {
    use mdv_msg::testutil::{tags, MsgBuilder};
    let html = "<html><body><table><tr><td>a</td><td>b</td></tr><tr><td>1</td><td>2</td></tr></table></body></html>";
    MsgBuilder::new().string(tags::SUBJECT, "Relatório mensal").binary(tags::HTML, html.as_bytes()).build()
}

#[test]
fn msg_no_modo_plain_mostra_cabecalho_e_tabela() {
    let d = tempfile::TempDir::new().unwrap();
    let p = msg_file(&d, "mail.msg", &email_com_tabela());
    let out = Command::new(env!("CARGO_BIN_EXE_mdv")).args(["-p", "-w", "80", &p]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Relatório mensal"), "{text}");
    assert!(text.contains('│'), "{text}");
}

#[test]
fn msg_exporta_markdown() {
    let d = tempfile::TempDir::new().unwrap();
    let p = msg_file(&d, "mail.msg", &email_com_tabela());
    let saida = d.path().join("out.md").to_string_lossy().into_owned();
    let out = Command::new(env!("CARGO_BIN_EXE_mdv")).args([&p, "-o", &saida]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let md = std::fs::read_to_string(&saida).unwrap();
    assert!(md.starts_with("# "), "{md}");
    let again = Command::new(env!("CARGO_BIN_EXE_mdv")).args([&p, "-o", &saida]).output().unwrap();
    assert_ne!(again.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&again.stderr).contains("já existe"), "{}", String::from_utf8_lossy(&again.stderr));
}

#[test]
fn msg_invalido_sai_com_erro_e_codigo_1() {
    let d = tempfile::TempDir::new().unwrap();
    let p = msg_file(&d, "ruim.msg", b"n\xc3\xa3o sou ole2");
    let out = Command::new(env!("CARGO_BIN_EXE_mdv")).args(["-p", &p]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("não é um arquivo .msg válido"));
}

#[test]
fn msg_com_limite_de_memoria_baixo_nao_trava() {
    use mdv_msg::testutil::{tags, MsgBuilder};
    let d = tempfile::TempDir::new().unwrap();
    let html = format!("<html><body>{}</body></html>", "<b>".repeat(1024 * 1024 / 3));
    let bytes = MsgBuilder::new().string(tags::SUBJECT, "Grande").binary(tags::HTML, html.as_bytes()).build();
    let p = msg_file(&d, "grande.msg", &bytes);
    let out = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(["-p", &p])
        .env("MDV_MEMORY_BUFFER", "32M")
        .output()
        .unwrap();
    // Sucesso ou erro de memória são aceitos; o que não pode acontecer é morrer por sinal.
    assert!(out.status.code().is_some(), "terminou por sinal: {:?}", out.status);
}

#[test]
fn html_em_plain() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.html");
    std::fs::write(&p, "<h1>Oi</h1><p style=\"color:red\">x</p>").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_mdv"))
        .args(["-p", "-w", "40"])
        .arg(&p)
        .output()
        .expect("mdv deve terminar");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let saida = String::from_utf8_lossy(&out.stdout);
    // Saída em pipe não tem cor (só em TTY); a cor do HTML é coberta em `src/html.rs`.
    assert!(saida.contains("Oi") && saida.contains('x') && !saida.contains('\x1b'), "{saida}");
}
