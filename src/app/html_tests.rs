//! Integração do HTML na TUI: `]`/`[` entre páginas (deck), títulos, links, `m`, busca e redimensionamento.

use super::*;
use crate::doc::raw_lang;
use unicode_width::UnicodeWidthStr;

/// `App` aberto sobre um documento HTML (`Doc::Html`), com janela de 3 linhas.
fn app_html(html_src: &str) -> App {
    let cli = cli::parse(["pagina.html".to_string()]).expect("cli de teste");
    let mut a = App::new(Source::Stdin, html_src.to_string(), 80, &cli);
    a.doc.opts.doc = cli::Doc::Html;
    a.rerender(60);
    a.view.height = 3;
    a
}

fn key(a: &mut App, c: KeyCode) -> bool {
    a.on_key(KeyEvent::new(c, KeyModifiers::NONE))
}

fn digita(a: &mut App, texto: &str) {
    for c in texto.chars() {
        key(a, KeyCode::Char(c));
    }
}

/// Texto de uma linha renderizada (sem estilos).
fn texto_da_linha(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Largura em colunas de uma linha renderizada (emoji e CJK contam 2).
fn largura_da_linha(l: &Line) -> usize {
    l.spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum()
}

/// Deck de quatro `<section>`, cada uma com um `<h1>` distinto e dois parágrafos.
fn deck() -> String {
    let secoes: String = ["Primeiro", "Segundo", "Terceiro", "Quarto"]
        .iter()
        .map(|t| format!("<section><h1>{t}</h1><p>texto de {t}</p><p>mais um parágrafo</p></section>"))
        .collect();
    format!("<main data-mdv-deck>{secoes}</main>")
}

#[test]
fn html_deck_navega_com_colchetes() {
    let mut a = app_html(&deck());
    let h = a.doc.rendered.headings.clone();
    assert!(h.len() >= 4, "um título por página: {h:?}");
    assert!(h.windows(2).all(|w| w[0] < w[1]), "{h:?}");
    let max = a.view.max_scroll(a.total_lines());

    // `]` leva ao início da página seguinte: cresce e cai num título do documento
    let mut passos = vec![a.view.scroll];
    for _ in 0..3 {
        key(&mut a, KeyCode::Char(']'));
        passos.push(a.view.scroll);
    }
    assert!(passos.windows(2).all(|w| w[0] < w[1]), "a rolagem cresce: {passos:?}");
    for &p in &passos[1..] {
        assert!(h.contains(&p) || p == max, "{p} não é título nem fim: {h:?}");
    }

    // `[` volta uma página
    key(&mut a, KeyCode::Char('['));
    assert_eq!(a.view.scroll, passos[2], "[ volta ao título anterior");
    key(&mut a, KeyCode::Char('['));
    assert_eq!(a.view.scroll, passos[1]);

    // no fim, `]` não passa da última página
    for _ in 0..10 {
        key(&mut a, KeyCode::Char(']'));
    }
    let fim = a.view.scroll;
    assert!(fim <= max, "rolagem {fim} passou do fim {max}");
    key(&mut a, KeyCode::Char(']'));
    assert_eq!(a.view.scroll, fim, "no fim, ] fica onde está");
}

#[test]
fn html_titulos_viram_headings() {
    let a = app_html("<h1>Início</h1><p>texto</p><h2>Primeiro</h2><p>a</p><h2>Segundo</h2><p>b</p>");
    let h = &a.doc.rendered.headings;
    assert_eq!(h.len(), 3, "{h:?}");
    assert!(h.windows(2).all(|w| w[0] < w[1]), "{h:?}");
    assert_eq!(a.doc.rendered.outline.len(), 3);
    let niveis: Vec<u8> = a.doc.rendered.outline.iter().map(|e| e.level).collect();
    assert_eq!(niveis, [1u8, 2, 2]);
}

#[test]
fn html_link_externo_pede_confirmacao() {
    let mut a = app_html("<p><a href=\"https://exemplo.dev\">x</a></p>");
    assert_eq!(a.doc.rendered.links, vec!["https://exemplo.dev".to_string()]);
    let marcado = a
        .doc
        .rendered
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .any(|s| render::link_id(&s.style) == Some(0));
    assert!(marcado, "o texto do link carrega a marca");

    key(&mut a, KeyCode::Tab);
    assert!(a.link_at.is_some(), "Tab seleciona o link");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.pending_open(), Some("https://exemplo.dev"), "{:?}", a.notice);
    // cancela sem executar nada (`n`)
    key(&mut a, KeyCode::Char('n'));
    assert!(a.pending_open().is_none());
    assert!(a.notice.as_deref().is_some_and(|n| n.contains("cancelado")), "{:?}", a.notice);
}

#[test]
fn html_m_mostra_fonte() {
    let mut a = app_html("<h1>Oi</h1><p>corpo</p>");
    assert_eq!(raw_lang(&a.doc.opts.doc), "html");
    key(&mut a, KeyCode::Char('m'));
    assert!(a.raw);
    a.rerender(60);
    assert_eq!(raw_lang(&a.doc.opts.doc), "html");
    let fonte: String = a.doc.rendered.lines.iter().map(texto_da_linha).collect::<Vec<_>>().join("\n");
    assert!(fonte.contains("<h1>"), "a fonte mostra a tag: {fonte}");

    key(&mut a, KeyCode::Char('m'));
    assert!(!a.raw);
    a.rerender(60);
    let vista: String = a.doc.rendered.lines.iter().map(texto_da_linha).collect::<Vec<_>>().join("\n");
    assert!(vista.contains("Oi") && vista.contains("corpo"), "{vista}");
    assert!(!vista.contains("<h1>"), "voltou ao HTML interpretado");
}

#[test]
fn html_busca_acha_texto() {
    let mut a = app_html("<p>um canto de sabiá na mata</p><p>outra linha</p>");
    key(&mut a, KeyCode::Char('/'));
    digita(&mut a, "sabiá");
    key(&mut a, KeyCode::Enter);
    assert_eq!(a.search_input(), None, "Enter fecha o prompt");
    assert!(!a.search.matches.is_empty(), "achou o termo no HTML");
    assert_eq!(a.search.query, "sabiá");
}

#[test]
fn html_redimensiona_e_rerenderiza() {
    let frase = "palavra ".repeat(40);
    let mut a = app_html(&format!("<p>{frase}</p>"));
    let linhas_largas = a.doc.rendered.lines.len();

    // tela de 34 colunas: texto de 30 colunas (TUI_MARGIN = 4)
    a.prepare(34);
    assert_eq!(a.doc.render_width, 30);
    assert!(a.doc.rendered.lines.iter().all(|l| largura_da_linha(l) <= 30), "nenhuma linha passa de 30");
    let linhas_estreitas = a.doc.rendered.lines.len();
    assert!(linhas_estreitas > linhas_largas, "mais colunas a menos: mais linhas ({linhas_largas} -> {linhas_estreitas})");

    // tela de 54 colunas: texto de 50 colunas
    a.prepare(54);
    assert_eq!(a.doc.render_width, 50);
    assert!(a.doc.rendered.lines.iter().all(|l| largura_da_linha(l) <= 50));
    assert!(a.doc.rendered.lines.iter().any(|l| largura_da_linha(l) > 30), "a primeira linha usa a nova largura");
    assert!(a.doc.rendered.lines.len() < linhas_estreitas);
}
