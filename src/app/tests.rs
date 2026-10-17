//! Testes do `App` que cruzam várias partes (busca, zoom, links, stream, cópia, rodapé).

use super::search::{highlight_line, line_text};
use super::zoom::{columns_label, most_visible, step_index};
use super::*;
use std::fs;

#[test]
fn stdin_para_de_acumular_quando_a_memoria_acaba() {
    let (_tx, rx) = std::sync::mpsc::channel::<String>();
    let mut app = test_app(Source::Stdin, "").with_stream(rx);
    app.stop_stream_for_memory(None);
    assert!(app.stream.is_some(), "sem estouro, segue lendo");
    app.stop_stream_for_memory(Some(mem::MemoryLimit { used: 2, limit: 1 }));
    assert!(app.stream.is_none());
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("leitura interrompida")));
    assert!(!app.images.mermaid_held(), "libera os diagramas");
}

#[test]
fn destaque_com_busca_vazia_nao_altera_a_linha() {
    let line = Line::from(vec![Span::raw("abc"), Span::raw("def")]);
    assert_eq!(highlight_line(&line, ""), line);
}

#[test]
fn destaque_marca_cada_ocorrencia() {
    let line = Line::from("a-b-a");
    let out = highlight_line(&line, "a");
    let marked: Vec<&str> = out
        .spans
        .iter()
        .filter(|s| s.style.bg.is_some())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(marked, ["a", "a"]);
    assert_eq!(line_text(&out), "a-b-a");
}

#[test]
fn destaque_atravessa_trechos_estilizados() {
    let bold = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
    let line = Line::from(vec![
        Span::raw("foo"),
        Span::styled("bar", bold),
        Span::raw(" baz"),
    ]);
    let out = highlight_line(&line, "foobar");
    assert_eq!(line_text(&out), "foobar baz");
    let marked: String = out
        .spans
        .iter()
        .filter(|s| s.style.bg.is_some())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(marked, "foobar");
    // o trecho em negrito continua em negrito, agora também destacado
    assert!(out.spans.iter().any(|s| s.content == "bar"
        && s.style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
        && s.style.bg.is_some()));
}

#[test]
fn destaque_ignora_caixa_e_nao_desalinha_com_i_turco() {
    let line = Line::from("İx e X");
    let out = highlight_line(&line, "x");
    assert_eq!(line_text(&out), "İx e X");
    let marked: Vec<&str> = out
        .spans
        .iter()
        .filter(|s| s.style.bg.is_some())
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(marked, ["x", "X"]);
}

#[test]
fn busca_acha_linha_com_texto_dividido_entre_trechos() {
    let lines = vec![
        Line::from(vec![Span::raw("foo"), Span::raw("bar")]),
        Line::from("outra"),
    ];
    let mut s = SearchState::default();
    s.commit("FooBar".to_string(), &lines, 0);
    assert_eq!(s.matches, [0]);
}

fn test_app(source: Source, md: &str) -> App {
    let cli = cli::parse(["x.md".to_string()]).expect("cli de teste");
    App::new(source, md.to_string(), 80, &cli)
}

/// Texto de todas as células do buffer, uma linha por string.
fn screen_text(buf: &ratatui::buffer::Buffer) -> Vec<String> {
    let w = buf.area.width;
    (0..buf.area.height)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect()
}

/// Ponta a ponta: diagrama Mermaid nativo → tecla `i` → zoom desenhado → `Esc` volta.
#[test]
fn tecla_i_abre_o_zoom_do_diagrama_mermaid() {
    use ratatui::{backend::TestBackend, Terminal};
    if !crate::mermaid_native::fonts_available() {
        return;
    }
    let md = "# D\n\n```mermaid\nflowchart LR\n  A[Início] --> B[Fim]\n```\n";
    let mut app = test_app(Source::Stdin, md);
    app.images = image::ImageStore::new(None)
        .with_native_mermaid(true)
        .with_graphics(true);
    let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    assert_eq!(
        app.doc.rendered.mermaids.len(),
        1,
        "o diagrama virou imagem"
    );
    app.on_key(ev(KeyCode::Char('i')));
    assert!(app.img_zoom().is_some(), "{:?}", app.notice);
    app.prepare_img_zoom(100, 30, 8.0);
    term.draw(|f| app.draw(f)).unwrap();
    let text = screen_text(term.backend().buffer()).join("\n");
    assert!(text.contains("diagrama 1/1"), "{text}");
    app.on_key(ev(KeyCode::Char('+')));
    app.on_key(ev(KeyCode::Esc));
    assert!(app.img_zoom().is_none());
}

#[test]
fn tecla_i_sem_diagrama_avisa() {
    let mut app = test_app(Source::Stdin, "# só texto\n");
    app.prepare(80);
    app.on_key(ev(KeyCode::Char('i')));
    assert!(app.img_zoom().is_none());
    assert_eq!(app.notice.as_deref(), Some("nenhum diagrama na tela"));
}

/// Regressão da troca do ratatui: desenha a TUI num `TestBackend` e confere conteúdo, estilo e rolagem.
#[test]
fn tui_desenha_documento_no_test_backend() {
    use ratatui::{backend::TestBackend, Terminal};
    let md = "# Título\n\nTexto **forte** e `código`.\n\n- um\n- dois\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let mut app = test_app(Source::Stdin, md);
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let linhas = screen_text(&buf);
    let tela = linhas.join("\n");
    for esperado in ["Título", "forte", "código", "um", "dois", "fn main", "│"] {
        assert!(
            tela.contains(esperado),
            "faltou {esperado:?} na tela:\n{tela}"
        );
    }
    let fg_titulo = (0..buf.area.width)
        .find(|&x| buf[(x, 0)].symbol() != " ")
        .map(|x| buf[(x, 0)].fg);
    assert!(
        fg_titulo.is_some_and(|c| c != ratatui::style::Color::Reset),
        "título sem cor"
    );
    // redimensionar e redesenhar não pode entrar em pânico nem perder o conteúdo
    let mut pequeno = Terminal::new(TestBackend::new(30, 8)).unwrap();
    pequeno.draw(|f| app.draw(f)).unwrap();
    assert!(screen_text(pequeno.backend().buffer())
        .join("\n")
        .contains("Título"));
}

fn key(app: &mut App, c: KeyCode) {
    app.on_key(KeyEvent::new(c, KeyModifiers::NONE));
}

#[test]
fn tab_enter_segue_link_md_e_backspace_volta() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.doc.md");
    fs::write(&a, "# A\n\n[ir](b.md#segundo)\n").unwrap();
    fs::write(
        dir.path().join("b.md"),
        "# B\n\ntexto\n\n## Segundo\n\nfim\n",
    )
    .unwrap();
    let mut app = test_app(Source::File(a.clone()), "# A\n\n[ir](b.md#segundo)\n");
    app.view.height = 3;
    app.rerender(60);
    key(&mut app, KeyCode::Tab);
    assert!(app.link_at.is_some());
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.name, "b.md");
    app.rerender(60);
    app.apply_pending_jump();
    let seg = app.doc.rendered.outline[1].line;
    assert!(
        app.view.scroll <= seg && seg < app.view.scroll + 3,
        "rolou até a âncora"
    );
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.name, "a.doc.md");
    assert!(app.back.is_empty());
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.notice.as_deref(), Some("nada para voltar"));
}

#[test]
fn backspace_restaura_o_modo_do_documento_anterior() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("d.json");
    fs::write(&json, "{\"a\":1}\n").unwrap();
    let mut app = test_app(Source::Stdin, "# A\n");
    app.back.push(browse::Back::File {
        path: json.clone(),
        line: 0,
        doc: cli::Doc::Json { json_path: None, view: cli::JsonView::Auto },
    });
    key(&mut app, KeyCode::Backspace);
    assert!(matches!(app.doc.opts.doc, cli::Doc::Json { .. }));
    assert!(matches!(&app.source, Source::File(p) if *p == json));
}

#[test]
fn link_externo_so_abre_com_y_e_link_fora_da_pasta_e_recusado() {
    let md = "[x](https://exemplo.org) e [y](/etc/passwd.md)\n";
    let mut app = test_app(Source::Stdin, md);
    app.view.height = 5;
    app.rerender(60);
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.pending_open(), Some("https://exemplo.org"));
    key(&mut app, KeyCode::Char('n'));
    assert!(app.pending_open().is_none());
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Enter);
    assert!(app.pending_open().is_none());
    assert!(
        app.notice
            .as_deref()
            .unwrap_or("")
            .starts_with("não abri o link"),
        "{:?}",
        app.notice
    );
}

#[test]
fn marcas_de_link_nao_chegam_a_tela() {
    let mut app = test_app(Source::Stdin, "[x](#a)\n\n# a\n");
    app.view.height = 5;
    app.rerender(60);
    key(&mut app, KeyCode::Tab);
    let lines = app.screen_lines();
    assert!(lines
        .iter()
        .flat_map(|l| &l.spans)
        .all(|s| s.style.underline_color.is_none()));
    assert!(lines.iter().flat_map(|l| &l.spans).any(|s| s
        .style
        .add_modifier
        .contains(ratatui::style::Modifier::REVERSED)));
}

#[test]
fn seletor_abre_arquivo_e_backspace_volta_a_ele() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("um.md"), "# Um\n").unwrap();
    let mut app = test_app(Source::Stdin, "");
    app.modal = Modal::Picker(picker::Picker::scan(dir.path()).unwrap());
    for c in "um".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none());
    assert_eq!(app.name, "um.md");
    key(&mut app, KeyCode::Backspace);
    let p = app.picker().expect("voltou ao seletor");
    assert_eq!(p.query, "um", "a consulta é mantida");
    assert!(
        !app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        "q digita no filtro"
    );
    assert!(app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
}

#[test]
fn seletor_continua_na_tela_se_o_segundo_arquivo_falha() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("um.md"), "# Um\n").unwrap();
    fs::write(dir.path().join("ruim.md"), "# Ruim\n").unwrap();
    let mut app = test_app(Source::Stdin, "");
    app.modal = Modal::Picker(picker::Picker::scan(dir.path()).unwrap());
    for c in "um".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Backspace);
    let depth = app.back.len();
    for _ in 0..2 {
        key(&mut app, KeyCode::Backspace);
    }
    for c in "ruim".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    fs::remove_file(dir.path().join("ruim.md")).unwrap();
    key(&mut app, KeyCode::Enter);
    assert!(app.picker().is_some(), "o seletor deve continuar aberto");
    assert_eq!(app.name, "um.md");
    assert_eq!(app.back.len(), depth, "nada novo empilhado");
}

#[test]
fn stream_acumula_pedacos_e_marca_para_redesenhar() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut app = test_app(Source::Stdin, "").with_stream(rx);
    app.doc.dirty = false;
    tx.send("# um\n".to_string()).unwrap();
    tx.send("dois".to_string()).unwrap();
    app.drain_stream();
    assert_eq!(app.doc.md, "# um\ndois");
    assert!(app.doc.dirty);
}

#[test]
fn diagramas_esperam_o_fim_do_stdin_em_streaming() {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut app = test_app(Source::Stdin, "").with_stream(rx);
    assert!(
        app.images.mermaid_held(),
        "enquanto o texto chega, não dispara o diagrama"
    );
    tx.send("# x\n".to_string()).unwrap();
    app.drain_stream();
    assert!(app.images.mermaid_held());
    drop(tx);
    app.doc.dirty = false;
    app.drain_stream();
    assert!(
        !app.images.mermaid_held(),
        "entrada terminou: libera os diagramas"
    );
    assert!(app.doc.dirty, "redesenha para os diagramas aparecerem");
}

#[test]
fn barra_de_status_explica_que_as_teclas_pulam_de_titulo() {
    let app = test_app(Source::Stdin, "# a");
    let barra: String = app
        .status_line(120)
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(barra.contains("[ ] título"), "{barra}");
}

#[test]
fn stream_sem_novidade_nao_redesenha() {
    let (_tx, rx) = std::sync::mpsc::channel::<String>();
    let mut app = test_app(Source::Stdin, "").with_stream(rx);
    app.doc.dirty = false;
    app.drain_stream();
    assert!(!app.doc.dirty);
}

#[test]
fn stream_acompanha_o_fim_so_se_ja_estava_no_fim() {
    let (tx, rx) = std::sync::mpsc::channel();
    let long: String = (0..30).map(|i| format!("linha {i}\n\n")).collect();
    let mut app = test_app(Source::Stdin, &long).with_stream(rx);
    app.rerender(40);
    app.view.height = 5;

    app.view.scroll = 0;
    tx.send("mais\n".to_string()).unwrap();
    app.drain_stream();
    assert!(
        !app.stick_bottom,
        "leitor no meio do texto não deve ser arrastado"
    );

    app.view.scroll = app.view.max_scroll(app.total_lines());
    tx.send("fim\n".to_string()).unwrap();
    app.drain_stream();
    assert!(app.stick_bottom);
    app.rerender(40);
    app.apply_stick_bottom();
    assert_eq!(app.view.scroll, app.view.max_scroll(app.total_lines()));
    assert!(!app.stick_bottom);
}

fn app_kitty(dir: &std::path::Path) -> App {
    let img = ::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(
        8,
        8,
        ::image::Rgba([9, 9, 9, 255]),
    ));
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, ::image::ImageFormat::Png).unwrap();
    std::fs::write(dir.join("a.png"), png.into_inner()).unwrap();
    let md = dir.join("a.doc.md");
    std::fs::write(&md, "![x](a.png)\n").unwrap();
    let cli = cli::parse(["--graphics", "kitty", "a.doc.md"].map(String::from)).expect("cli de teste");
    App::new(Source::File(md), "![x](a.png)\n".to_string(), 80, &cli)
}

#[test]
fn prepare_enfileira_a_imagem_uma_vez_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_kitty(dir.path());
    app.prepare(100);
    assert!(
        app.pending_gfx.starts_with("\x1b_Ga=T"),
        "{:?}",
        app.pending_gfx
    );
    assert_eq!(app.sent_graphics.len(), 1);
    app.pending_gfx.clear();
    app.doc.dirty = true;
    app.prepare(100);
    assert!(app.pending_gfx.is_empty(), "já foi enviada");
}

#[test]
fn abrir_outro_arquivo_reenvia_as_imagens_com_ids_reiniciados() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_kitty(dir.path());
    app.prepare(100);
    app.pending_gfx.clear();
    let img = ::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(
        8,
        8,
        ::image::Rgba([200, 0, 0, 255]),
    ));
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, ::image::ImageFormat::Png).unwrap();
    std::fs::write(dir.path().join("b.png"), png.into_inner()).unwrap();
    let b = dir.path().join("b.doc.md");
    std::fs::write(&b, "![y](b.png)\n").unwrap();
    app.open_file(b, None, true, cli::Doc::Markdown);
    app.prepare(100);
    assert!(app.pending_gfx.starts_with(crate::graphics::DELETE_ALL), "{:?}", app.pending_gfx);
    assert!(app.pending_gfx.contains("\x1b_Ga=T"), "a imagem nova precisa ser enviada");
}

#[test]
fn sem_graphics_nada_e_enfileirado() {
    let mut app = test_app(Source::Stdin, "![x](nao.png)");
    app.prepare(100);
    assert!(app.pending_gfx.is_empty() && app.sent_graphics.is_empty());
}

fn tecla(app: &mut App, c: char) {
    app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn v_alterna_as_vistas_do_json() {
    let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
    app.doc.opts.doc = cli::Doc::Json { json_path: None, view: cli::JsonView::Auto };
    tecla(&mut app, 'v'); // Auto já era tabela: o próximo é o esquema
    assert_eq!(json_view(&app), cli::JsonView::Schema);
    assert!(app.doc.dirty);
    tecla(&mut app, 'v');
    assert_eq!(json_view(&app), cli::JsonView::Text);
    tecla(&mut app, 'v');
    assert_eq!(json_view(&app), cli::JsonView::Table);
}

#[test]
fn v_em_json_que_nao_e_lista_comeca_pela_tabela() {
    let mut app = test_app(Source::Stdin, r#"{"a":1}"#);
    app.doc.opts.doc = cli::Doc::Json { json_path: None, view: cli::JsonView::Auto };
    tecla(&mut app, 'v');
    assert_eq!(json_view(&app), cli::JsonView::Table);
}

#[test]
fn v_fora_do_modo_json_nao_faz_nada() {
    let mut app = test_app(Source::Stdin, "# a");
    app.doc.dirty = false;
    tecla(&mut app, 'v');
    assert_eq!(json_view(&app), cli::JsonView::Auto);
    assert!(!app.doc.dirty);
}

#[test]
fn barra_mostra_a_vista_do_json_quando_escolhida() {
    let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
    app.doc.opts.doc = cli::Doc::Json { json_path: None, view: cli::JsonView::Auto };
    tecla(&mut app, 'v');
    assert!(
        app.status_left().contains("[esquema]"),
        "{}",
        app.status_left()
    );
}

#[test]
fn w_liga_codigo_sem_quebra_e_hl_rolam() {
    let longa = "x".repeat(200);
    let mut app = test_app(Source::Stdin, &format!("```\n{longa}\n```"));
    app.rerender(60); // 58 colunas de código => overflow 142
    assert_eq!(app.doc.rendered.code_overflow, 142);
    tecla(&mut app, 'w');
    assert_eq!(app.doc.opts.code_scroll, Some(0));
    assert!(app.doc.dirty);
    app.rerender(60);
    tecla(&mut app, 'l');
    assert_eq!(app.doc.opts.code_scroll, Some(8));
    tecla(&mut app, 'h');
    tecla(&mut app, 'h');
    assert_eq!(app.doc.opts.code_scroll, Some(0));
    tecla(&mut app, '$');
    assert_eq!(app.doc.opts.code_scroll, Some(142));
    tecla(&mut app, '0');
    assert_eq!(app.doc.opts.code_scroll, Some(0));
    tecla(&mut app, 'w');
    assert_eq!(app.doc.opts.code_scroll, None);
}

#[test]
fn hl_sem_w_avisam_e_nao_mudam_nada() {
    let mut app = test_app(Source::Stdin, "```\nabc\n```");
    app.rerender(60);
    tecla(&mut app, 'l');
    assert_eq!(app.doc.opts.code_scroll, None);
    assert!(
        app.notice.as_deref().is_some_and(|n| n.contains('w')),
        "{:?}",
        app.notice
    );
}

#[test]
fn rolagem_nao_passa_do_fim_do_codigo() {
    let mut app = test_app(Source::Stdin, &format!("```\n{}\n```", "x".repeat(70)));
    app.rerender(60); // overflow 12
    tecla(&mut app, 'w');
    for _ in 0..5 {
        tecla(&mut app, 'l');
    }
    assert_eq!(app.doc.opts.code_scroll, Some(12));
}

#[test]
fn barra_de_status_mostra_o_modo_sem_quebra() {
    let mut app = test_app(Source::Stdin, "```\nabc\n```");
    app.rerender(60);
    tecla(&mut app, 'w');
    assert!(
        app.status_left().contains("sem quebra"),
        "{}",
        app.status_left()
    );
}

fn app_com_tabela(md: &str) -> App {
    let mut app = test_app(Source::Stdin, md);
    app.rerender(60);
    app.view.height = 10;
    app
}

fn ev(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn app_com_texto(md: &str) -> App {
    let mut app = test_app(Source::Stdin, md);
    app.rerender(60);
    app.view.height = 10;
    app
}

fn copiado(app: &App) -> Option<String> {
    app.pending_osc.clone()
}

#[test]
fn rodape_mostra_o_andamento_das_tarefas_do_documento() {
    let app = app_com_texto("- [x] a\n- [ ] b\n- [ ] c\n");
    assert!(app.status_left().contains("☑ 1/3"), "{}", app.status_left());
}

#[test]
fn rodape_sem_tarefas_nao_mostra_contador() {
    let app = app_com_texto("# A\n\ntexto\n");
    assert!(!app.status_left().contains('☑'), "{}", app.status_left());
}

#[test]
fn rodape_acrescenta_a_secao_atual_quando_ela_tem_tarefas() {
    let mut app = app_com_texto("# A\n\n- [x] a\n\n# B\n\n- [ ] b\n- [ ] c\n");
    app.view.scroll = app.doc.rendered.outline[1].line;
    let s = app.status_left();
    assert!(s.contains("☑ 1/3") && s.contains("(seção 0/2)"), "{s}");
}

#[test]
fn rodape_nao_repete_a_contagem_quando_a_secao_e_o_documento() {
    let app = app_com_texto("# A\n\n- [x] a\n- [ ] b\n");
    let s = app.status_left();
    assert!(s.contains("☑ 1/2") && !s.contains("seção"), "{s}");
}

#[test]
fn rodape_com_tarefa_antes_do_primeiro_titulo_nao_quebra() {
    let app = app_com_texto("- [x] solta\n\n# A\n\ntexto\n");
    assert!(app.status_left().contains("☑ 1/1"));
}

#[test]
fn rodape_no_modo_fonte_nao_mostra_contador() {
    let mut app = app_com_texto("- [x] a\n");
    app.on_key(ev(KeyCode::Char('m')));
    app.rerender(60);
    assert!(!app.status_left().contains('☑'), "{}", app.status_left());
}

#[test]
fn y_copia_o_bloco_de_codigo_da_tela() {
    let mut app = app_com_texto("texto\n\n```rust\nfn a() {}\n```\n");
    app.on_key(ev(KeyCode::Char('y')));
    assert_eq!(copiado(&app), Some(clip::osc52("fn a() {}\n").unwrap()));
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("copiado: 1 linha")),
        "{:?}",
        app.notice
    );
}

#[test]
fn y_sem_bloco_de_codigo_avisa_e_nao_copia() {
    let mut app = app_com_texto("só texto\n");
    app.on_key(ev(KeyCode::Char('y')));
    assert_eq!(copiado(&app), None);
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("nenhum bloco de código")));
}

#[test]
fn y_em_json_nao_entra_em_panico_e_avisa() {
    let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
    app.doc.opts.doc = cli::Doc::Json { json_path: None, view: cli::JsonView::Auto };
    app.rerender(60);
    app.view.height = 10;
    app.on_key(ev(KeyCode::Char('y')));
    assert_eq!(copiado(&app), None);
    assert!(app.notice.is_some());
}

#[test]
fn y_no_modo_fonte_copia_o_documento() {
    let mut app = app_com_texto("# A\n\ntexto\n");
    app.on_key(ev(KeyCode::Char('m')));
    app.rerender(60);
    app.on_key(ev(KeyCode::Char('y')));
    assert_eq!(copiado(&app), Some(clip::osc52("# A\n\ntexto\n").unwrap()));
}

#[test]
fn y_maiusculo_copia_o_markdown_cru_da_secao() {
    let mut app = app_com_texto("# A\n\ntexto\n\n## B\n\nb\n");
    app.on_key(ev(KeyCode::Char('Y')));
    assert_eq!(
        copiado(&app),
        Some(clip::osc52("# A\n\ntexto\n\n## B\n\nb\n").unwrap())
    );
}

#[test]
fn y_maiusculo_usa_a_secao_da_primeira_linha_visivel() {
    let mut app = app_com_texto("# A\n\na\n\n# B\n\nb\n");
    let b = app.doc.rendered.outline[1].line;
    app.view.scroll = b;
    app.on_key(ev(KeyCode::Char('Y')));
    assert_eq!(copiado(&app), Some(clip::osc52("# B\n\nb\n").unwrap()));
}

#[test]
fn y_maiusculo_sem_titulo_avisa_e_nao_copia() {
    let mut app = app_com_texto("só texto\n");
    app.on_key(ev(KeyCode::Char('Y')));
    assert_eq!(copiado(&app), None);
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("nenhuma seção")));
}

#[test]
fn copia_maior_que_64_kib_nao_envia_nada() {
    let md = format!("```\n{}\n```\n", "x".repeat(70 * 1024));
    let mut app = app_com_texto(&md);
    app.on_key(ev(KeyCode::Char('y')));
    assert_eq!(copiado(&app), None);
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("grande demais (70 KiB)")),
        "{:?}",
        app.notice
    );
}

const TABELA: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";

fn tabela_larga(n: usize) -> String {
    let h: String = (1..=n).map(|i| format!(" c{i} |")).collect();
    format!("|{h}\n|{}\n|{h}\n", "---|".repeat(n))
}

#[test]
fn mais_visivel_prefere_o_bloco_com_mais_linhas_na_tela() {
    // janela 10..30: bloco 0 mostra 5 linhas (5..15), bloco 1 mostra 20 (15..60)
    assert_eq!(
        most_visible([(5, 15), (15, 60)].into_iter(), 10..30),
        Some(1)
    );
    assert_eq!(most_visible([(100, 120)].into_iter(), 10..30), None);
    assert_eq!(
        most_visible([(10, 20), (20, 30)].into_iter(), 10..30),
        Some(0),
        "empate: o primeiro"
    );
}

#[test]
fn n_e_p_trocam_de_tabela_no_zoom_dando_a_volta() {
    let md = format!("{TABELA}\n{}\n{TABELA}", "texto\n\n".repeat(30));
    let mut app = app_com_tabela(&md);
    app.on_key(ev(KeyCode::Char('t')));
    app.on_key(ev(KeyCode::Char('p')));
    assert_eq!(
        app.zoom().map(|z| z.table),
        Some(1),
        "do primeiro volta ao último"
    );
    app.on_key(ev(KeyCode::Char('n')));
    assert_eq!(
        app.zoom().map(|z| z.table),
        Some(0),
        "do último vai ao primeiro"
    );
    assert_eq!(step_index(0, 1, true), None);
}

#[test]
fn t_abre_o_zoom_da_tabela_visivel() {
    let mut app = app_com_tabela(TABELA);
    assert!(!app.on_key(ev(KeyCode::Char('t'))));
    assert_eq!(app.zoom().map(|z| z.table), Some(0));
}

#[test]
fn t_sem_tabela_na_tela_avisa_e_nao_abre() {
    let mut app = app_com_tabela("# só texto\n");
    app.on_key(ev(KeyCode::Char('t')));
    assert!(app.zoom().is_none());
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("nenhuma tabela")));
    app.on_key(ev(KeyCode::Char('j')));
    assert!(app.notice.is_none(), "o aviso some na próxima tecla");
}

#[test]
fn t_escolhe_a_tabela_que_aparece_na_tela() {
    let md = format!("{TABELA}\n{}\n{TABELA}", "texto\n\n".repeat(30));
    let mut app = app_com_tabela(&md);
    let segunda = app.doc.rendered.tables[1].start;
    app.view.scroll = segunda.saturating_sub(1);
    app.on_key(ev(KeyCode::Char('t')));
    assert_eq!(app.zoom().map(|z| z.table), Some(1));
}

#[test]
fn esc_e_q_fecham_o_zoom_sem_sair() {
    for k in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('t')] {
        let mut app = app_com_tabela(TABELA);
        app.on_key(ev(KeyCode::Char('t')));
        assert!(!app.on_key(ev(k)), "{k:?} no zoom só fecha");
        assert!(app.zoom().is_none());
    }
}

#[test]
fn ctrl_c_sai_mesmo_no_zoom() {
    let mut app = app_com_tabela(TABELA);
    app.on_key(ev(KeyCode::Char('t')));
    assert!(app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
}

#[test]
fn colunas_rolam_com_h_l_zero_e_cifrao() {
    let mut app = app_com_tabela(&tabela_larga(12));
    app.on_key(ev(KeyCode::Char('t')));
    app.refresh_zoom(20);
    let first = |app: &App| app.zoom().unwrap().first_col;
    assert_eq!(first(&app), 1);

    app.on_key(ev(KeyCode::Char('l')));
    app.refresh_zoom(20);
    assert_eq!(first(&app), 2);

    app.on_key(ev(KeyCode::Char('$')));
    app.refresh_zoom(20);
    let v = app.zoom().unwrap().drawn.as_ref().map(|d| &d.2).unwrap();
    assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols);

    app.on_key(ev(KeyCode::Right));
    app.refresh_zoom(20);
    let v = app.zoom().unwrap().drawn.as_ref().map(|d| &d.2).unwrap();
    assert_eq!(
        v.first_col + v.shown_cols - 1,
        v.total_cols,
        "não passa da última"
    );

    app.on_key(ev(KeyCode::Char('0')));
    app.refresh_zoom(20);
    assert_eq!(first(&app), 1);

    app.on_key(ev(KeyCode::Left));
    app.refresh_zoom(20);
    assert_eq!(first(&app), 1, "não anda para antes da primeira");
}

#[test]
fn rolagem_vertical_do_zoom_nao_passa_do_fim() {
    let linhas: String = (0..40).map(|i| format!("| {i} |\n")).collect();
    let mut app = app_com_tabela(&format!("| n |\n|---|\n{linhas}"));
    app.on_key(ev(KeyCode::Char('t')));
    app.refresh_zoom(30);
    app.on_key(ev(KeyCode::Char('G')));
    let z = app.zoom().unwrap();
    assert!(z.view.scroll > 0);
    app.on_key(ev(KeyCode::Char('g')));
    assert_eq!(app.zoom().unwrap().view.scroll, 0);
}

#[test]
fn recarregar_com_a_tabela_sumida_fecha_o_zoom() {
    let mut app = app_com_tabela(TABELA);
    app.on_key(ev(KeyCode::Char('t')));
    app.doc.md = "sem tabela\n".to_string();
    app.rerender(60);
    assert!(app.zoom().is_none());
}

#[test]
fn rerender_com_a_tabela_ainda_presente_mantem_o_zoom_e_refaz_o_desenho() {
    let mut app = app_com_tabela(TABELA);
    app.on_key(ev(KeyCode::Char('t')));
    app.refresh_zoom(30);
    app.rerender(60);
    let z = app.zoom().expect("continua aberto");
    assert!(z.drawn.is_none(), "o cache do desenho é descartado");
}

#[test]
fn rotulo_de_colunas() {
    let md = tabela_larga(12);
    let out = render::render(&md, 20);
    let data = &out.tables[0].data;
    assert_eq!(columns_label(&data.draw(200, 0)), "todas as 12 colunas");
    assert_eq!(columns_label(&data.draw(20, 0)), "colunas 1 e 2–3 de 12");
    let so_uma = render::render("| a |\n|---|\n| 1 |\n", 20);
    assert_eq!(
        columns_label(&so_uma.tables[0].data.draw(20, 0)),
        "1 coluna"
    );
}

fn app_com_cli(args: &[&str], md: &str) -> App {
    let cli = cli::parse(args.iter().map(|s| s.to_string())).expect("cli de teste");
    App::new(Source::Stdin, md.to_string(), 80, &cli)
}

#[test]
fn table_n_abre_o_zoom_na_primeira_renderizacao() {
    let md = format!("{TABELA}\ntexto\n\n{TABELA}");
    let mut app = app_com_cli(&["--table", "2", "-"], &md);
    app.rerender(60);
    app.open_pending_table();
    assert_eq!(app.zoom().map(|z| z.table), Some(1));
    assert!(app.open_table.is_none());
}

#[test]
fn table_alem_do_que_existe_avisa_e_nao_abre() {
    let mut app = app_com_cli(&["--table", "5", "-"], TABELA);
    app.rerender(60);
    app.open_pending_table();
    assert!(app.zoom().is_none());
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("tabela 5 não existe (há 1)")));
}

/// Spec 5: `main.rs` enxuto; nenhum arquivo novo acima de ~500 linhas de produção.
#[test]
fn arquivos_de_producao_dentro_do_teto() {
    let arquivos: &[(&str, &str, usize)] = &[
        ("main.rs", include_str!("../main.rs"), 120),
        ("load.rs", include_str!("../load.rs"), 500),
        ("plain.rs", include_str!("../plain.rs"), 500),
        ("doc.rs", include_str!("../doc.rs"), 500),
        ("app/mod.rs", include_str!("mod.rs"), 500),
        ("app/viewport.rs", include_str!("viewport.rs"), 500),
        ("app/search.rs", include_str!("search.rs"), 500),
        ("app/zoom.rs", include_str!("zoom.rs"), 500),
        ("app/modal.rs", include_str!("modal.rs"), 500),
        ("app/input.rs", include_str!("input.rs"), 500),
        ("app/ui.rs", include_str!("ui.rs"), 500),
        ("app/terminal.rs", include_str!("terminal.rs"), 500),
    ];
    for (nome, src, teto) in arquivos {
        let producao = src.split("#[cfg(test)]\nmod tests {").next().unwrap().lines().count();
        assert!(producao <= *teto, "{nome} tem {producao} linhas de produção (teto {teto})");
    }
}

fn json_view(app: &App) -> cli::JsonView {
    match &app.doc.opts.doc {
        cli::Doc::Json { view, .. } => *view,
        _ => cli::JsonView::Auto,
    }
}
