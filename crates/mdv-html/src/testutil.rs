//! Auxiliares de teste compartilhadas pelos módulos de layout (só compilam em `cargo test`).

use crate::css;
use crate::flow::layout;
use crate::guard::Limits;
use crate::index;
use crate::model::{Cell, Face, Line};
use crate::style::{compute, root_style, Cascade};
use crate::Unlimited;
use scraper::{Html, Selector};

pub(crate) struct Saida {
    pub linhas: Vec<Line>,
    pub links: Vec<String>,
    pub titulos: Vec<(usize, u8, String)>,
}

/// Layout do `body` de `html` com `css_src`, na largura `width`. O fundo do `html` é `None`.
pub(crate) fn rodar_com(
    html: &str,
    css_src: &str,
    width: usize,
    lim: &Limits,
    budget: &dyn crate::Budget,
) -> Result<Saida, String> {
    let doc = Html::parse_document(html);
    let idx = index::build(css::parse_sheet(css_src, 800, lim));
    let cascade = Cascade { index: &idx, lim };
    let cx = crate::ctx::Ctx::new(&cascade, budget);
    let raiz = compute(doc.root_element(), &root_style(), &cascade);
    let body = doc.select(&Selector::parse("body").unwrap()).next().unwrap();
    let bs = compute(body, &raiz, &cascade);
    let linhas = layout(body, &bs, width, None, &raiz, 0, &cx)?;
    let links = cx.links.borrow().clone();
    let titulos = cx.headings.borrow().clone();
    Ok(Saida {
        linhas,
        links,
        titulos,
    })
}

/// Texto de cada linha, aparado à direita.
pub(crate) fn render_body(html: &str, css_src: &str, width: usize) -> Vec<String> {
    let saida = rodar_com(html, css_src, width, &Limits::default(), &Unlimited).unwrap();
    saida.linhas.iter().map(|l| texto(l)).collect()
}

pub(crate) fn texto(l: &[Cell]) -> String {
    l.iter().map(|c| c.ch).collect::<String>().trim_end().to_string()
}

/// Estilos de todas as células com o caractere `ch`, em ordem.
pub(crate) fn faces(linhas: &[Line], ch: char) -> Vec<Face> {
    linhas
        .iter()
        .flat_map(|l| l.iter())
        .filter(|c| c.ch == ch)
        .map(|c| c.face)
        .collect()
}
