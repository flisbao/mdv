//! Listas do HTML do Outlook: cada item é a conversão do filho direto; o recuo segue a largura do marcador.

use crate::html::{Ctx, Flow};
use scraper::ElementRef;

/// Maior início aceito em `<ol start>`, para a soma com os itens não estourar.
const MAX_INICIO_LISTA: usize = 999_999_000;

/// Lista: cada filho direto é um item; itens vazios somem. O recuo de um item é a largura do marcador.
pub(crate) fn lista(c: &Ctx<'_>, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let ordenada = el.value().name() == "ol";
    let inicio = el
        .attr("start")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .min(MAX_INICIO_LISTA);
    let mut itens: Vec<String> = Vec::new();
    for filho in el.child_elements() {
        c.check()?;
        let mut f = Flow::new(c, cell, true);
        f.element(filho, depth + 1)?;
        let blocos = f.finish();
        if blocos.is_empty() {
            continue;
        }
        let marca = if ordenada { format!("{}. ", inicio.saturating_add(itens.len())) } else { "- ".to_string() };
        let recuo = marca.len();
        let mut item = String::new();
        for (k, bl) in blocos.iter().enumerate() {
            if k == 0 {
                item.push_str(&marca);
                item.push_str(&recua(bl, false, recuo));
            } else {
                item.push_str("\n\n");
                item.push_str(&recua(bl, true, recuo));
            }
        }
        c.gasta(item.len());
        itens.push(item);
    }
    Ok(if itens.is_empty() { vec![] } else { vec![itens.join("\n")] })
}

/// Recuo nas linhas de um bloco de item de lista (a primeira só se `todas`).
pub(crate) fn recua(s: &str, todas: bool, recuo: usize) -> String {
    let pad = " ".repeat(recuo);
    s.split('\n')
        .enumerate()
        .map(|(i, l)| {
            if (i == 0 && !todas) || l.is_empty() {
                l.to_string()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
