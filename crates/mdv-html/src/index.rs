//! Índice de regras por id, classe, tag e "qualquer", para não testar a folha inteira em cada elemento.

use crate::css::{Rule, Sheet};
use crate::selector::{Key, Pseudo};
use scraper::ElementRef;
use std::collections::HashMap;

/// Baldes de índices de regras para um conjunto de regras (normais, `::before` ou `::after`).
/// Cada regra entra em um único balde, então um elemento não recebe a mesma regra duas vezes.
#[derive(Debug, Default)]
struct Baldes {
    por_id: HashMap<String, Vec<usize>>,
    por_classe: HashMap<String, Vec<usize>>,
    por_tag: HashMap<String, Vec<usize>>,
    qualquer: Vec<usize>,
}

impl Baldes {
    fn adiciona(&mut self, chave: Key, i: usize) {
        match chave {
            Key::Id(s) => self.por_id.entry(s).or_default().push(i),
            Key::Class(s) => self.por_classe.entry(s).or_default().push(i),
            Key::Tag(s) => self.por_tag.entry(s).or_default().push(i),
            Key::Any => self.qualquer.push(i),
        }
    }

    /// Acrescenta em `out` os índices de regras que podem casar com o elemento.
    fn candidatos(&self, el: ElementRef<'_>, out: &mut Vec<usize>) {
        let v = el.value();
        if let Some(b) = v.id().and_then(|id| self.por_id.get(id)) {
            out.extend_from_slice(b);
        }
        // Classes repetidas no atributo não podem repetir o balde.
        let mut classes: Vec<&str> = v.classes().collect();
        classes.sort_unstable();
        classes.dedup();
        for c in classes {
            if let Some(b) = self.por_classe.get(c) {
                out.extend_from_slice(b);
            }
        }
        if let Some(b) = self.por_tag.get(v.name()) {
            out.extend_from_slice(b);
        }
        out.extend_from_slice(&self.qualquer);
    }
}

/// Índice pronto para casar regras com elementos.
#[derive(Debug, Default)]
pub struct Index {
    regras: Vec<Rule>,
    normais: Baldes,
    antes: Baldes,
    depois: Baldes,
}

/// Monta o índice a partir da folha.
pub fn build(sheet: Sheet) -> Index {
    let mut idx = Index::default();
    for (i, r) in sheet.rules.iter().enumerate() {
        let balde = match r.pseudo {
            None => &mut idx.normais,
            Some(Pseudo::Before) => &mut idx.antes,
            Some(Pseudo::After) => &mut idx.depois,
        };
        balde.adiciona(r.selector.key(), i);
    }
    idx.regras = sheet.rules;
    idx
}

impl Index {
    /// Regras normais (sem pseudoelemento) que casam com o elemento, ordenadas por (especificidade, ordem).
    pub fn matching<'a>(&'a self, el: ElementRef<'_>) -> Vec<&'a Rule> {
        self.casam(&self.normais, el)
    }

    /// Regras do pseudoelemento dado que casam com o elemento, na mesma ordenação.
    pub fn matching_pseudo<'a>(&'a self, el: ElementRef<'_>, p: Pseudo) -> Vec<&'a Rule> {
        let balde = match p {
            Pseudo::Before => &self.antes,
            Pseudo::After => &self.depois,
        };
        self.casam(balde, el)
    }

    fn casam<'a>(&'a self, balde: &Baldes, el: ElementRef<'_>) -> Vec<&'a Rule> {
        let mut candidatos = Vec::new();
        balde.candidatos(el, &mut candidatos);
        let mut out: Vec<&Rule> = candidatos
            .into_iter()
            .filter_map(|i| self.regras.get(i))
            .filter(|r| r.selector.matches(el))
            .collect();
        out.sort_by_cached_key(|r| (r.selector.specificity(), r.order));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::parse_sheet;
    use crate::guard::Limits;
    use scraper::Html;

    fn nos<'a>(doc: &'a Html, css: &str) -> Vec<ElementRef<'a>> {
        doc.select(&scraper::Selector::parse(css).unwrap()).collect()
    }

    fn indice(css: &str) -> Index {
        build(parse_sheet(css, 800, &Limits::default()))
    }

    #[test]
    fn ordem_por_especificidade_e_ordem() {
        let idx = indice("a{color:red} .x{color:blue} a{color:green}");
        let doc = Html::parse_document(r#"<a class="x">t</a>"#);
        let el = nos(&doc, "a")[0];
        let ordens: Vec<usize> = idx.matching(el).iter().map(|r| r.order).collect();
        assert_eq!(ordens, vec![0, 2, 1]);
    }

    #[test]
    fn so_regras_do_elemento() {
        let idx = indice(".y{color:red} .x{color:blue}");
        let doc = Html::parse_document(r#"<div class="x">t</div>"#);
        let el = nos(&doc, "div")[0];
        let casam = idx.matching(el);
        assert_eq!(casam.len(), 1);
        assert_eq!(casam[0].order, 1);
    }

    #[test]
    fn baldes_de_id_e_tag() {
        let idx = indice("#i{color:red} p{color:blue}");
        let doc = Html::parse_document(r#"<p id="i">t</p><p>u</p>"#);
        let ps = nos(&doc, "p");
        assert_eq!(idx.matching(ps[0]).len(), 2);
        assert_eq!(idx.matching(ps[1]).len(), 1);
    }

    #[test]
    fn pseudo_separado() {
        let idx = indice(".x::before{content:\"a\"}");
        let doc = Html::parse_document(r#"<div class="x">t</div>"#);
        let el = nos(&doc, "div")[0];
        assert!(idx.matching(el).is_empty());
        let antes = idx.matching_pseudo(el, Pseudo::Before);
        assert_eq!(antes.len(), 1);
        assert_eq!(antes[0].decls[0].name, "content");
        assert!(idx.matching_pseudo(el, Pseudo::After).is_empty());
    }
}
