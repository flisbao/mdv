//! Renderização completa: HTML → `Document` (páginas de células). Os testes ficam no fim do arquivo.
//! Uma página é o `body` inteiro, ou cada filho de um deck (`deck.rs`). Todas as páginas usam
//! um único `Ctx`, então links são compartilhados e títulos recebem a página em que estão.

use crate::color;
use crate::ctx::Ctx;
use crate::css;
use crate::deck::{estilo_com_cadeia, find_deck};
use crate::flow::layout;
use crate::guard::{self, Limits};
use crate::index;
use crate::inline;
use crate::model::{Document, Heading, Line, Page, Rgb};
use crate::style::{compute, root_style, Cascade, Display};
use crate::Budget;
use scraper::{ElementRef, Html};

/// Tamanho máximo, em caracteres, do título de uma página.
const MAX_TITULO_PAGINA: usize = 60;
/// Tamanho máximo, em caracteres, do `<title>` do documento.
const MAX_TITULO_DOC: usize = 200;

/// Renderiza `html` em páginas de `width` colunas (já limitada a 20..=400 pelo chamador).
/// `viewport_px` decide as condições de `@media`. Erros de teto viram `Err` com a mensagem.
pub fn render(
    html: &str,
    width: usize,
    viewport_px: u32,
    lim: &Limits,
    budget: &dyn Budget,
) -> Result<Document, String> {
    budget.check()?;
    guard::check_html(html, lim)?;
    let doc = Html::parse_document(html);
    let raiz = doc.root_element();
    let idx = index::build(css::parse_sheet(&colhe_estilos(raiz, lim), viewport_px, lim));
    let cascade = Cascade { index: &idx, lim };
    let Some(body) = raiz
        .descendants()
        .filter_map(ElementRef::wrap)
        .find(|e| e.value().name() == "body")
    else {
        return Ok(Document::default());
    };
    let hs = compute(raiz, &root_style(), &cascade);
    let bs = compute(body, &hs, &cascade);
    let fundo_html = inline::fundo_efetivo(&hs, None);
    let fundo_pag = inline::fundo_efetivo(&bs, fundo_html);
    let largura = width.clamp(20, 400);
    let cx = Ctx::new(&cascade, budget);
    let mut paginas: Vec<Page> = Vec::new();
    let mut titulos: Vec<Heading> = Vec::new();

    match find_deck(body, &cascade) {
        Some(filhos) => {
            for filho in filhos.into_iter().take(lim.max_pages) {
                budget.check()?;
                let pai_st = match filho.parent().and_then(ElementRef::wrap) {
                    Some(pai) => estilo_com_cadeia(pai, &cascade),
                    None => bs.clone(),
                };
                let mut st = compute(filho, &pai_st, &cascade);
                // Slide é uma coluna: flex/grid viram flex em coluna; o resto vira bloco.
                st.display = match st.display {
                    Display::Flex | Display::Grid => {
                        st.flex_column = true;
                        Display::Flex
                    }
                    _ => Display::Block,
                };
                st.hidden = false;
                st.faded = false;
                st.fixed = false;
                let linhas = layout(filho, &st, largura, fundo_pag, &pai_st, 0, &cx)?;
                let indice = paginas.len();
                let (pagina, cabecas) = fecha(filho, linhas, fundo_pag, &cx, indice, lim)?;
                paginas.push(pagina);
                titulos.extend(cabecas);
            }
        }
        None => {
            budget.check()?;
            let linhas = layout(body, &bs, largura, fundo_html, &hs, 0, &cx)?;
            let (pagina, cabecas) = fecha(body, linhas, fundo_pag, &cx, 0, lim)?;
            paginas.push(pagina);
            titulos.extend(cabecas);
        }
    }

    let links = cx.links.borrow().clone();
    Ok(Document {
        title: titulo_documento(raiz),
        pages: paginas,
        links,
        headings: titulos,
    })
}

/// Fecha uma página: esvazia os títulos registrados por `cx` (com a página `indice`), pinta o
/// fundo da página em toda célula sem fundo, corrige o contraste e apara as linhas em branco.
fn fecha(
    el: ElementRef<'_>,
    linhas: Vec<Line>,
    fundo: Option<Rgb>,
    cx: &Ctx<'_>,
    indice: usize,
    lim: &Limits,
) -> Result<(Page, Vec<Heading>), String> {
    let brutos: Vec<(usize, u8, String)> = cx.headings.borrow_mut().drain(..).collect();
    let (linhas, antes) = aparada(preenchida(linhas, fundo), fundo);
    if linhas.len() > lim.max_lines {
        return Err("página longa demais".to_string());
    }
    let titulo = titulo_pagina(el).or_else(|| primeira_linha(&linhas));
    let cabecas = brutos
        .into_iter()
        .map(|(linha, nivel, texto)| Heading {
            page: indice,
            line: linha.saturating_sub(antes),
            level: nivel,
            text: texto,
        })
        .collect();
    Ok((Page { title: titulo, lines: linhas }, cabecas))
}

/// Pinta de `fundo` as células sem fundo próprio e corrige o contraste do texto (não mexe em
/// espaço puro).
fn preenchida(mut linhas: Vec<Line>, fundo: Option<Rgb>) -> Vec<Line> {
    for l in linhas.iter_mut() {
        for c in l.iter_mut() {
            if c.face.bg.is_none() {
                c.face.bg = fundo;
            }
            if c.ch != ' ' {
                c.face.fg = color::fix_contrast(c.face.fg, c.face.bg);
            }
        }
    }
    linhas
}

/// Remove linhas em branco repetidas nas pontas, deixando no máximo uma em cada ponta. Só são
/// removidas linhas sem fundo próprio (fundo igual ao da página). Devolve quantas saíram do início.
fn aparada(mut linhas: Vec<Line>, fundo: Option<Rgb>) -> (Vec<Line>, usize) {
    let vazia = |l: &Line| l.iter().all(|c| c.ch == ' ' && c.face.bg == fundo);
    let ini = linhas.iter().take_while(|l| vazia(l)).count();
    let antes = ini.saturating_sub(1);
    linhas.drain(..antes);
    let fim = linhas.iter().rev().take_while(|l| vazia(l)).count();
    let tirar = fim.saturating_sub(1);
    let n = linhas.len();
    linhas.truncate(n.saturating_sub(tirar));
    (linhas, antes)
}

/// Primeira linha com texto visível, como título de último recurso.
fn primeira_linha(linhas: &[Line]) -> Option<String> {
    linhas
        .iter()
        .find_map(|l| limpa(&l.iter().map(|c| c.ch).collect::<String>(), MAX_TITULO_PAGINA))
}

/// Título da página: primeiro `h1`, depois `h2`, depois `.title` dentro de `el`.
fn titulo_pagina(el: ElementRef<'_>) -> Option<String> {
    primeiro_com(el, |e| e.value().name() == "h1")
        .or_else(|| primeiro_com(el, |e| e.value().name() == "h2"))
        .or_else(|| primeiro_com(el, |e| e.value().classes().any(|c| c == "title")))
}

/// Texto saneado do primeiro descendente (ou o próprio `el`) que satisfaz `casa`.
fn primeiro_com<'a>(el: ElementRef<'a>, casa: impl Fn(&ElementRef<'a>) -> bool) -> Option<String> {
    el.descendants()
        .filter_map(ElementRef::wrap)
        .find(|e| casa(e))
        .and_then(|e| limpa(&e.text().collect::<String>(), MAX_TITULO_PAGINA))
}

/// `<title>` do documento, saneado.
fn titulo_documento(raiz: ElementRef<'_>) -> Option<String> {
    raiz.descendants()
        .filter_map(ElementRef::wrap)
        .find(|e| e.value().name() == "title")
        .and_then(|e| limpa(&e.text().collect::<String>(), MAX_TITULO_DOC))
}

/// Concatena o conteúdo de todos os `<style>` (até o teto de CSS).
fn colhe_estilos(raiz: ElementRef<'_>, lim: &Limits) -> String {
    let mut css_src = String::new();
    for e in raiz
        .descendants()
        .filter_map(ElementRef::wrap)
        .filter(|e| e.value().name() == "style")
    {
        for pedaco in e.text() {
            css_src.push_str(pedaco);
            css_src.push('\n');
        }
        if css_src.len() > lim.max_css_bytes {
            break;
        }
    }
    css_src
}

/// Tira controles e marcas de direção do texto, junta espaços e corta em `max` caracteres
/// (com reticências). Devolve `None` se nada sobrar.
fn limpa(s: &str, max: usize) -> Option<String> {
    let limpo: String = s.chars().filter(|c| !c.is_control() && !bidi(*c)).collect();
    let t = limpo.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return None;
    }
    if t.chars().count() <= max {
        return Some(t);
    }
    let mut corte: String = t.chars().take(max.saturating_sub(1)).collect();
    corte.push('…');
    Some(corte)
}

/// Marcas de direção do texto (sobrescrita e isolamento bidirecional).
fn bidi(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::color;
    use crate::guard::Limits;
    use crate::model::{line_width, Document};
    use crate::{Budget, Unlimited};

    fn doc(html: &str) -> Document {
        render(html, 40, 320, &Limits::default(), &Unlimited).unwrap()
    }

    fn texto_pagina(d: &Document, i: usize) -> String {
        d.pages[i]
            .lines
            .iter()
            .map(|l| l.iter().map(|c| c.ch).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    struct Falha;

    impl Budget for Falha {
        fn check(&self) -> Result<(), String> {
            Err("orçamento estourado".to_string())
        }
    }

    const DECK_CLASSE: &str = "<html><head><style>.slide{position:absolute;opacity:0}\
        .slide.active{opacity:1}</style></head><body><div id=\"deck\">\
        <div class=\"slide active\"><h2>Um</h2></div>\
        <div class=\"slide\"><h2>Dois</h2></div>\
        <div class=\"slide\"><h2>Três</h2></div>\
        <div class=\"slide\"><h2>Quatro</h2></div></div></body></html>";

    #[test]
    fn pagina_comum_tem_uma_pagina() {
        let d = doc("<p>um</p><p>dois</p>");
        assert_eq!(d.pages.len(), 1);
    }

    #[test]
    fn deck_por_classe() {
        let d = doc(DECK_CLASSE);
        assert_eq!(d.pages.len(), 4);
        assert!(texto_pagina(&d, 3).contains("Quatro"));
    }

    #[test]
    fn deck_por_section_com_atributo() {
        let d = doc(
            "<main data-mdv-deck><section><p>a</p></section><section><p>b</p></section>\
             <section><p>c</p></section></main>",
        );
        assert_eq!(d.pages.len(), 3);
    }

    #[test]
    fn tres_paragrafos_nao_sao_deck() {
        let d = doc("<p class=\"x\">a</p><p class=\"x\">b</p><p class=\"x\">c</p>");
        assert_eq!(d.pages.len(), 1);
    }

    #[test]
    fn deck_exige_um_escondido_ou_atributo() {
        let sem = doc("<div id=\"c\"><div class=\"s\">a</div><div class=\"s\">b</div>\
            <div class=\"s\">c</div></div>");
        assert_eq!(sem.pages.len(), 1);
        let com = doc("<div id=\"c\" data-mdv-deck><div class=\"s\">a</div>\
            <div class=\"s\">b</div><div class=\"s\">c</div></div>");
        assert_eq!(com.pages.len(), 3);
    }

    #[test]
    fn titulo_da_pagina() {
        let d = doc(
            "<main data-mdv-deck><section><h2>Agenda</h2><p>x</p></section>\
             <section><p>sem título</p></section><section><h2>\
             aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\
             </h2></section></main>",
        );
        assert_eq!(d.pages[0].title.as_deref(), Some("Agenda"));
        assert_eq!(d.pages[1].title.as_deref(), Some("sem título"));
        let longo = d.pages[2].title.clone().unwrap_or_default();
        assert!(longo.chars().count() <= 60, "{longo:?}");
    }

    #[test]
    fn fundo_do_body_atravessa() {
        let d = doc("<html><head><style>body{background:#101010}</style></head>\
            <body><p>oi</p><div style=\"padding:12px 0\"><p>dentro</p></div>\
            <p style=\"color:red\">cor</p></body></html>");
        assert!(!d.pages[0].lines.is_empty());
        for l in &d.pages[0].lines {
            for c in l {
                assert!(c.face.bg.is_some(), "célula sem fundo: {:?}", c.ch);
            }
        }
    }

    #[test]
    fn contraste_corrigido() {
        let d = doc("<html><head><style>body{background:#fff;color:#fff}</style></head>\
            <body><p>texto visível</p><p style=\"color:#fefefe\">quase</p></body></html>");
        let mut testadas = 0;
        for l in &d.pages[0].lines {
            for c in l.iter().filter(|c| c.ch != ' ') {
                let fg = c.face.fg.expect("texto sem cor");
                let bg = c.face.bg.expect("texto sem fundo");
                assert!(color::contrast(fg, bg) >= 2.0, "{:?}", c);
                testadas += 1;
            }
        }
        assert!(testadas > 0);
    }

    #[test]
    fn titulo_do_documento() {
        let d = doc("<html><head><title>  Relatório \n anual </title></head><body><p>x</p></body></html>");
        assert_eq!(d.title.as_deref(), Some("Relatório anual"));
    }

    #[test]
    fn teto_de_paginas() {
        let lim = Limits {
            max_pages: 2,
            ..Limits::default()
        };
        let mut html = String::from("<main data-mdv-deck>");
        for i in 0..5 {
            html.push_str(&format!("<section><p>slide {i}</p></section>"));
        }
        html.push_str("</main>");
        let d = render(&html, 40, 320, &lim, &Unlimited).unwrap();
        assert_eq!(d.pages.len(), 2);
    }

    #[test]
    fn erro_html_aninhado() {
        let err = render(&"<div>".repeat(6000), 40, 320, &Limits::default(), &Unlimited).unwrap_err();
        assert!(err.contains("aninhado"), "{err}");
    }

    #[test]
    fn mensagem_de_budget() {
        let err = render("<p>x</p>", 40, 320, &Limits::default(), &Falha).unwrap_err();
        assert_eq!(err, "orçamento estourado");
    }

    #[test]
    fn headings_absolutos() {
        let d = doc(
            "<main data-mdv-deck><section><p>a</p></section>\
             <section><p>b</p><h1>Fim</h1></section><section><p>c</p></section></main>",
        );
        assert_eq!(d.pages.len(), 3);
        let h = d.headings.iter().find(|h| h.text == "Fim").unwrap();
        assert_eq!(h.page, 1);
        assert_eq!(h.level, 1);
        let linha: String = d.pages[1].lines[h.line].iter().map(|c| c.ch).collect();
        assert!(linha.contains("Fim"), "{linha:?}");
    }

    #[test]
    fn larguras_respeitadas() {
        let d = doc("<h1>Título bem comprido de teste</h1><p>日本語 😀 texto texto texto</p>");
        for l in &d.pages[0].lines {
            assert_eq!(line_width(l), 40);
        }
    }

    #[test]
    fn amostras_do_corpus() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/html");
        let mut nomes: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "html"))
            .collect();
        nomes.sort();
        assert!(!nomes.is_empty());
        let mut falhas = Vec::new();
        for caminho in &nomes {
            let html = std::fs::read_to_string(caminho).unwrap();
            for w in [60usize, 80, 120] {
                let r = render(&html, w, 0, &Limits::default(), &Unlimited);
                match r {
                    Ok(d) => {
                        let linhas: usize = d.pages.iter().map(|p| p.lines.len()).sum();
                        if linhas == 0 {
                            falhas.push(format!("{} w={w}: sem linhas", caminho.display()));
                        }
                        for p in &d.pages {
                            for l in &p.lines {
                                if line_width(l) > w {
                                    falhas.push(format!("{} w={w}: linha larga", caminho.display()));
                                }
                            }
                        }
                    }
                    Err(e) => falhas.push(format!("{} w={w}: {e}", caminho.display())),
                }
            }
        }
        assert!(falhas.is_empty(), "{falhas:#?}");
    }
}
