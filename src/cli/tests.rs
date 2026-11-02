use super::*;

fn run(args: &[&str]) -> Result<Cli, String> {
    parse(args.iter().map(|s| s.to_string()))
}

fn doc(args: &[&str]) -> Doc {
    match run(args).unwrap().command {
        Command::View(d) => d,
        outro => panic!("{outro:?}"),
    }
}

fn tabela(args: &[&str]) -> (u8, usize, Vec<String>, bool) {
    match doc(args) {
        Doc::Table { delimiter, head, cols, no_header } => (delimiter, head, cols, no_header),
        outro => panic!("{outro:?}"),
    }
}

fn json_view(args: &[&str]) -> JsonView {
    match doc(args) {
        Doc::Json { view, .. } => view,
        outro => panic!("{outro:?}"),
    }
}

#[test]
fn graphics_tem_padrao_auto_e_aceita_kitty_e_off() {
    assert_eq!(run(&["a.md"]).unwrap().common.graphics, Graphics::Auto);
    assert_eq!(run(&["--graphics", "kitty", "a.md"]).unwrap().common.graphics, Graphics::Kitty);
    assert_eq!(run(&["--graphics", "auto", "a.md"]).unwrap().common.graphics, Graphics::Auto);
    assert_eq!(run(&["--graphics", "off", "a.md"]).unwrap().common.graphics, Graphics::Off);
}

#[test]
fn graphics_invalido_ou_sem_valor_e_erro() {
    assert!(run(&["--graphics", "sixel", "a.md"]).unwrap_err().contains("sixel"));
    assert!(run(&["--graphics"]).is_err());
}

#[test]
fn as_define_a_vista_do_json() {
    assert_eq!(json_view(&["json", "d.json", "--as", "table"]), JsonView::Table);
    assert_eq!(json_view(&["json", "d.json", "--as", "schema"]), JsonView::Schema);
    assert_eq!(json_view(&["json", "d.json", "--as", "text"]), JsonView::Text);
    assert_eq!(json_view(&["json", "d.json", "--as", "auto"]), JsonView::Auto);
    assert_eq!(json_view(&["d.json"]), JsonView::Auto);
}

#[test]
fn as_por_extensao_tambem_vale() {
    assert_eq!(json_view(&["d.jsonl", "--as", "table"]), JsonView::Table);
}

#[test]
fn as_invalido_ou_fora_do_json_e_erro() {
    assert!(run(&["json", "--as", "xml", "-"]).unwrap_err().contains("xml"));
    assert!(run(&["--as"]).is_err());
    assert!(run(&["a.md", "--as", "table"]).unwrap_err().contains("--as"));
    assert!(run(&["csv", "a.csv", "--as", "table"]).unwrap_err().contains("--as"));
}

#[test]
fn markdown_por_padrao() {
    let c = run(&["a.md"]).unwrap();
    assert_eq!(doc(&["a.md"]), Doc::Markdown);
    assert_eq!(c.common.path.as_deref(), Some("a.md"));
}

#[test]
fn subcomando_json_com_caminho() {
    let c = run(&["json", "d.json", "--path", ".a.b[0]"]).unwrap();
    assert_eq!(
        c.command,
        Command::View(Doc::Json { json_path: Some(".a.b[0]".into()), view: JsonView::Auto })
    );
}

#[test]
fn extensao_define_modo() {
    assert_eq!(tabela(&["x.csv"]).0, b',');
    assert_eq!(tabela(&["x.tsv"]).0, b'\t');
    assert!(matches!(doc(&["x.jsonl"]), Doc::Json { .. }));
}

#[test]
fn tsv_subcomando_le_stdin() {
    let c = run(&["tsv", "-"]).unwrap();
    assert!(matches!(c.command, Command::View(Doc::Table { delimiter: b'\t', .. })));
    assert_eq!(c.common.path.as_deref(), Some("-"));
}

#[test]
fn largura_invalida_e_erro() {
    assert!(run(&["-w", "abc", "a.md"]).is_err());
    assert!(run(&["-w"]).is_err());
    assert!(run(&["-w", "-3"]).is_err());
    assert!(run(&["-w", ""]).is_err());
    assert!(run(&["-w", "abc"]).unwrap_err().contains("abc"));
}

#[test]
fn largura_e_limitada() {
    assert_eq!(run(&["-w", "99999", "a.md"]).unwrap().common.width, Some(1000));
    assert_eq!(run(&["-w", "1", "a.md"]).unwrap().common.width, Some(20));
    assert_eq!(run(&["-w", "18446744073709551615"]).unwrap().common.width, Some(1000));
    assert_eq!(run(&["--width", "80", "-"]).unwrap().common.width, Some(80));
}

#[test]
fn flag_desconhecida_e_erro() {
    assert!(run(&["--nada", "a.md"]).unwrap_err().contains("--nada"));
}

#[test]
fn mais_de_um_arquivo_e_erro() {
    assert!(run(&["a.md", "b.md"]).is_err());
    assert!(run(&["--", "a", "b"]).is_err());
}

#[test]
fn duplo_traco_aceita_caminho_com_hifen() {
    assert_eq!(run(&["--", "-x.md"]).unwrap().common.path.as_deref(), Some("-x.md"));
    assert!(!run(&["--", "-p"]).unwrap().common.plain);
}

#[test]
fn sem_argumentos_usa_padroes() {
    let c = run(&[]).unwrap();
    assert!(!c.common.plain && c.common.width.is_none() && c.common.path.is_none());
}

#[test]
fn colunas_separadas_por_virgula() {
    assert_eq!(tabela(&["csv", "a.csv", "--cols", "id,nome"]).2, vec!["id", "nome"]);
}

#[test]
fn delimitador_e_head_validados() {
    let (d, head, _, no_header) = tabela(&["csv", "a.csv", "--delimiter", ";", "--head", "5", "--no-header"]);
    assert_eq!((d, head, no_header), (b';', 5, true));
    assert!(run(&["csv", "a.csv", "--delimiter", "ab"]).is_err());
    assert!(run(&["csv", "a.csv", "--head", "x"]).unwrap_err().contains("--head"));
}

#[test]
fn ajuda_e_versao() {
    assert!(matches!(parse(["-h".to_string()]), Ok(Cli { command: Command::Help, .. })));
    assert!(matches!(parse(["a.md".to_string(), "-V".to_string()]), Ok(Cli { command: Command::Version, .. })));
}

    #[test]
    fn memory_buffer_aceita_tamanhos() {
        assert_eq!(run(&["--memory-buffer", "256M", "a.md"]).unwrap().common.memory_buffer, Some(256 * 1024 * 1024));
        assert_eq!(run(&["--memory-buffer", "1g", "a.md"]).unwrap().common.memory_buffer, Some(1024 * 1024 * 1024));
        assert_eq!(run(&["a.md"]).unwrap().common.memory_buffer, None);
    }

    #[test]
    fn memory_buffer_invalido_e_erro_de_uso() {
        for v in ["0", "-1", "1.5G", "abc", "", "16M", "999999999999999999999G"] {
            let e = run(&["--memory-buffer", v, "a.md"]).unwrap_err();
            assert!(e.contains("--memory-buffer"), "{v:?}: {e}");
        }
        assert!(run(&["--memory-buffer"]).is_err());
    }

    #[test]
    fn table_aceita_numero_a_partir_de_1() {
        assert_eq!(run(&["--table", "2", "a.md"]).unwrap().common.table, Some(2));
        assert_eq!(run(&["a.md"]).unwrap().common.table, None);
    }

    #[test]
    fn table_invalido_e_erro_de_uso() {
        for v in ["0", "-1", "x", ""] {
            let e = run(&["--table", v, "a.md"]).unwrap_err();
            assert!(e.contains("--table"), "{e}");
        }
        assert!(run(&["--table"]).is_err());
    }

#[test]
fn valor_faltando_no_fim_mantem_a_mensagem() {
    assert_eq!(run(&["a.md", "-w"]).unwrap_err(), "a opção -w precisa de uma largura");
    assert_eq!(run(&["a.md", "--path"]).unwrap_err(), "a opção --path precisa de um valor");
    assert_eq!(run(&["a.md", "--head"]).unwrap_err(), "a opção --head precisa de um valor");
    assert_eq!(run(&["a.md", "--cols"]).unwrap_err(), "a opção --cols precisa de um valor");
}

#[test]
fn delimiter_forca_o_modo_tabela_mesmo_com_extensao_md() {
    assert_eq!(tabela(&["--delimiter", ";", "a.md"]).0, b';');
}

#[test]
fn delimiter_tab_e_barra_t() {
    for v in ["tab", "\\t"] {
        assert_eq!(tabela(&["--delimiter", v, "a.txt"]).0, b'\t');
    }
    assert_eq!(run(&["--delimiter", "ab", "a.txt"]).unwrap_err(), "delimitador inválido: ab");
}

#[test]
fn head_e_limitado_ao_intervalo() {
    assert_eq!(tabela(&["a.csv", "--head", "0"]).1, 1);
    assert_eq!(tabela(&["a.csv", "--head", "999999999"]).1, MAX_HEAD);
    assert_eq!(run(&["a.csv", "--head", "x"]).unwrap_err(), "--head inválido: x");
}

#[test]
fn caminho_duplicado_e_graphics_invalido_mantem_mensagem() {
    assert_eq!(run(&["a.md", "b.md"]).unwrap_err(), "informe um único arquivo");
    assert_eq!(
        run(&["--graphics", "x", "a.md"]).unwrap_err(),
        "--graphics inválido: x (use off, kitty ou auto)"
    );
}

#[test]
fn idle_foi_removido() {
    let e = run(&["--follow", "--idle", "5"]).unwrap_err();
    assert!(e.contains("opção desconhecida: --idle"), "{e}");
}

#[test]
fn stats_nao_combina_com_toc_follow() {
    for f in ["--toc", "--follow"] {
        let e = run(&["--stats", f, "a.md"]).unwrap_err();
        assert!(e.contains("--stats") && e.contains(f), "{f}: {e}");
    }
}

#[test]
fn stats_so_vale_para_markdown() {
    let e = run(&["csv", "--stats", "a.csv"]).unwrap_err();
    assert!(e.contains("--stats"), "{e}");
    assert!(run(&["--stats", "a.md"]).is_ok());
}

#[test]
fn flags_de_outro_formato_sao_erro() {
    let casos: &[(&[&str], &str)] = &[
        (&["a.md", "--head", "5"], "a opção --head só vale para CSV/TSV"),
        (&["a.md", "--cols", "x"], "a opção --cols só vale para CSV/TSV"),
        (&["a.md", "--no-header"], "a opção --no-header só vale para CSV/TSV"),
        (&["a.json", "--head", "5"], "a opção --head só vale para CSV/TSV"),
        (&["a.csv", "--path", ".x"], "a opção --path só vale para JSON e YAML"),
        (&["a.md", "--path", ".x"], "a opção --path só vale para JSON e YAML"),
        (&["a.csv", "--as", "text"], "a opção --as só vale para JSON e YAML"),
        (&["a.json", "--toc"], "--toc só vale para Markdown"),
        (&["a.csv", "--toc"], "--toc só vale para Markdown"),
    ];
    for (args, msg) in casos {
        assert_eq!(run(args).unwrap_err(), *msg, "{args:?}");
    }
}

#[test]
fn delimiter_em_markdown_vira_tabela_e_aceita_flags_de_tabela() {
    let cli = run(&["--delimiter", ";", "--head", "3", "a.txt"]).unwrap();
    match cli.command {
        Command::View(Doc::Table { delimiter, head, .. }) => assert_eq!((delimiter, head), (b';', 3)),
        outro => panic!("{outro:?}"),
    }
}

#[test]
fn comandos_toc_stats_help_version() {
    assert!(matches!(run(&["--toc", "a.md"]).unwrap().command, Command::Toc));
    assert!(matches!(run(&["--stats", "a.md"]).unwrap().command, Command::Stats));
    assert!(matches!(run(&["-h"]).unwrap().command, Command::Help));
    assert!(matches!(run(&["a.md", "-V"]).unwrap().command, Command::Version));
}

fn export(args: &[&str]) -> Export {
    match run(args).unwrap().command {
        Command::Export(e) => e,
        outro => panic!("{outro:?}"),
    }
}

#[test]
fn o_vira_export_com_o_formato_da_entrada() {
    let e = export(&["a.md", "-o", "a.pdf"]);
    assert_eq!((e.doc, e.output.as_str(), e.force, e.section), (Doc::Markdown, "a.pdf", false, None));
    let e = export(&["--output", "t.xlsx", "d.csv", "--force", "--no-header"]);
    assert_eq!(e.output, "t.xlsx");
    assert!(e.force);
    assert!(matches!(e.doc, Doc::Table { delimiter: b',', no_header: true, .. }));
    let e = export(&["tsv", "-", "-o", "t.ods"]);
    assert!(matches!(e.doc, Doc::Table { delimiter: b'\t', .. }));
}

#[test]
fn saida_md_so_com_msg_docx_e_xlsx() {
    let e = run(&["a.md", "-o", "b.md"]).unwrap_err();
    assert!(e.contains("só vale para .msg, .docx e .xlsx"), "{e}");
    let e = run(&["a.csv", "-o", "b.md"]).unwrap_err();
    assert!(e.contains("só vale para .msg, .docx e .xlsx"), "{e}");
    assert!(run(&["-", "-o", "b.md"]).unwrap_err().contains("só vale para .msg, .docx e .xlsx"));
    let c = run(&["a.MSG", "-o", "b.md"]).unwrap();
    assert!(matches!(c.command, Command::Export(Export { doc: Doc::Markdown, .. })));
    let c = run(&["a.docx", "-o", "b.md"]).unwrap();
    assert!(matches!(c.command, Command::Export(Export { doc: Doc::Markdown, .. })));
    let c = run(&["a.xlsx", "-o", "b.md"]).unwrap();
    assert!(matches!(c.command, Command::Export(Export { doc: Doc::Markdown, .. })));
    assert!(run(&["a.msg", "-o", "b.md", "--section", "X"]).unwrap_err().contains("não combina"));
    assert!(run(&["a.msg", "-o", "b.md", "--table", "1"]).unwrap_err().contains("não combina"));
}

#[test]
fn export_guarda_secao_e_table_fica_em_common() {
    let e = export(&["a.md", "-o", "t.xlsx", "--section", "Vendas"]);
    assert_eq!(e.section.as_deref(), Some("Vendas"));
    let c = run(&["a.md", "-o", "t.xlsx", "--table", "2"]).unwrap();
    assert_eq!(c.common.table, Some(2));
}

#[test]
fn force_e_section_exigem_o() {
    assert!(run(&["a.md", "--force"]).unwrap_err().contains("--force só vale com -o"));
    assert!(run(&["a.md", "--section", "A"]).unwrap_err().contains("--section só vale com -o"));
}

#[test]
fn o_nao_combina_com_toc_stats_follow() {
    for f in ["--toc", "--stats", "--follow"] {
        let e = run(&["a.md", "-o", "a.pdf", f]).unwrap_err();
        assert!(e.contains("-o") && e.contains(f), "{f}: {e}");
    }
}

#[test]
fn section_so_para_markdown_e_nao_junto_de_table() {
    assert!(run(&["d.csv", "-o", "t.xlsx", "--section", "A"]).unwrap_err().contains("--section só vale para Markdown"));
    assert!(run(&["a.md", "-o", "t.xlsx", "--section", "A", "--table", "1"]).unwrap_err().contains("--section não combina com --table"));
}

#[test]
fn head_e_cols_sao_recusados_com_o() {
    for f in [["--head", "5"], ["--cols", "a"]] {
        let e = run(&["d.csv", "-o", "t.xlsx", f[0], f[1]]).unwrap_err();
        assert!(e.contains(f[0]) && e.contains("-o"), "{e}");
    }
    assert!(run(&["d.csv", "--head", "5"]).is_ok(), "sem -o continua valendo");
}

#[test]
fn yaml_deduzido_da_extensao() {
    for a in ["x.yaml", "x.YML"] {
        assert_eq!(doc(&[a]), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
    }
}

#[test]
fn subcomando_yaml_com_caminho_e_vista() {
    assert_eq!(
        doc(&["yaml", "d.yml", "--path", ".a", "--as", "table"]),
        Doc::Yaml { yaml_path: Some(".a".into()), view: JsonView::Table }
    );
    assert_eq!(doc(&["yaml", "-"]), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
}

#[test]
fn opcoes_de_csv_em_yaml_sao_erro() {
    assert_eq!(run(&["a.yaml", "--head", "5"]).unwrap_err(), "a opção --head só vale para CSV/TSV");
    assert_eq!(run(&["a.yaml", "--toc"]).unwrap_err(), "--toc só vale para Markdown");
}

#[test]
fn yaml_nao_exporta() {
    assert!(run(&["a.yaml", "-o", "x.xlsx"]).unwrap_err().contains("YAML ainda não exporta"));
}

#[test]
fn doc_for_path_devolve_o_padrao_de_cada_formato() {
    assert_eq!(doc_for_path("a.md"), Doc::Markdown);
    assert_eq!(doc_for_path("semextensao"), Doc::Markdown);
    assert_eq!(doc_for_path("a.json"), Doc::Json { json_path: None, view: JsonView::Auto });
    assert_eq!(doc_for_path("a.YAML"), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
    assert_eq!(doc_for_path("a.yml"), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
    assert_eq!(
        doc_for_path("a.tsv"),
        Doc::Table { delimiter: b'\t', head: DEFAULT_HEAD, cols: vec![], no_header: false }
    );
}

#[test]
fn data_view_expoe_caminho_e_vista_de_json_e_yaml() {
    let mut d = doc_for_path("a.yaml");
    let (path, view) = d.data_view_mut().unwrap();
    *path = Some(".x".into());
    *view = JsonView::Schema;
    assert_eq!(d.data_view(), Some((&Some(".x".to_string()), JsonView::Schema)));
    assert!(doc_for_path("a.json").data_view().is_some());
    assert!(doc_for_path("a.md").data_view().is_none());
    assert!(doc_for_path("a.csv").data_view_mut().is_none());
}

#[test]
fn doc_for_path_html() {
    for a in ["a.html", "a.htm", "a.HTM", "a.xhtml"] {
        assert_eq!(doc_for_path(a), Doc::Html, "{a}");
    }
    assert_eq!(doc(&["página.HTML"]), Doc::Html);
}

#[test]
fn html_nao_exporta() {
    let e = run(&["a.html", "-o", "x.pdf"]).unwrap_err();
    assert_eq!(e, "HTML ainda não exporta (use .md, .csv ou .tsv como saída)");
}

#[test]
fn toc_e_stats_so_markdown_inclui_html() {
    assert_eq!(run(&["a.html", "--toc"]).unwrap_err(), "--toc só vale para Markdown");
    assert_eq!(run(&["a.htm", "--stats"]).unwrap_err(), "--stats só vale para Markdown");
}

#[test]
fn head_em_html_e_erro() {
    assert_eq!(run(&["a.html", "--head", "5"]).unwrap_err(), "a opção --head só vale para CSV/TSV");
}

#[test]
fn ajuda_cita_html() {
    assert!(crate::plain::HELP.contains("HTML"));
}

#[test]
fn json_nao_exporta() {
    assert!(run(&["d.json", "-o", "t.xlsx"]).unwrap_err().contains("JSON"));
}

#[test]
fn o_sem_valor_ou_vazio_e_erro() {
    assert!(run(&["a.md", "-o"]).is_err());
    assert!(run(&["a.md", "-o", ""]).unwrap_err().contains("-o"));
}

#[test]
fn o_com_valor_que_parece_flag_e_erro() {
    for a in [&["a.md", "-o", "--force"][..], &["a.md", "-o", "-p"]] {
        let e = run(a).unwrap_err();
        assert!(e.contains("a opção -o precisa de um nome de arquivo"), "{e}");
    }
}

#[test]
fn opcoes_comuns_valem_em_todos_os_formatos() {
    for arq in ["a.md", "a.json", "a.csv"] {
        let c = run(&["-p", "-w", "50", "--table", "2", "--follow", arq]).unwrap().common;
        assert!(c.plain && c.follow);
        assert_eq!((c.width, c.table), (Some(50), Some(2)));
    }
}
