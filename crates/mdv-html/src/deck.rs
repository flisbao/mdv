//! Detecção de apresentações (deck): um contêiner cujos filhos repetem a mesma classe (ou a mesma
//! tag `section`/`article`), com pelo menos um filho escondido pelo CSS, ou marcado com
//! `data-mdv-deck`. Cada filho vira uma página. Na dúvida, não é deck.

use crate::style::{compute, root_style, Cascade, Style};
use scraper::ElementRef;
use std::collections::{BTreeMap, VecDeque};

/// Profundidade máxima (a partir do `body`) dos contêineres examinados.
const PROFUNDIDADE_DECK: usize = 6;
/// Elementos visitados (e filhos examinados por contêiner) no máximo na busca.
const MAX_NOS_DECK: usize = 20_000;

/// Estilo calculado de `el` a partir da cadeia de ancestrais (`html` → … → `el`).
pub(crate) fn estilo_com_cadeia(el: ElementRef<'_>, cx: &Cascade<'_>) -> Style {
    let mut cadeia: Vec<ElementRef<'_>> = el.ancestors().filter_map(ElementRef::wrap).collect();
    cadeia.reverse();
    cadeia.push(el);
    let mut pai = root_style();
    for a in cadeia {
        pai = compute(a, &pai, cx);
    }
    pai
}

/// Procura o deck a partir de `body` (largura até `PROFUNDIDADE_DECK`). Devolve os filhos do
/// contêiner escolhido (o de mais filhos), na ordem do documento, inclusive os escondidos.
pub fn find_deck<'a>(body: ElementRef<'a>, cascade: &Cascade<'a>) -> Option<Vec<ElementRef<'a>>> {
    let st_body = estilo_com_cadeia(body, cascade);
    let mut fila: VecDeque<(ElementRef<'a>, Style, usize)> = VecDeque::new();
    fila.push_back((body, st_body, 0));
    let mut visitados = 0usize;
    let mut melhor: Option<Vec<ElementRef<'a>>> = None;
    while let Some((el, st, prof)) = fila.pop_front() {
        visitados += 1;
        if visitados > MAX_NOS_DECK {
            break;
        }
        let filhos: Vec<(ElementRef<'a>, Style)> = el
            .children()
            .filter_map(ElementRef::wrap)
            .take(MAX_NOS_DECK)
            .map(|c| {
                let cs = compute(c, &st, cascade);
                (c, cs)
            })
            .collect();
        if let Some(grupo) = melhor_grupo(el, &filhos) {
            if melhor.as_ref().is_none_or(|m| grupo.len() > m.len()) {
                melhor = Some(grupo);
            }
        }
        if prof < PROFUNDIDADE_DECK {
            for (c, cs) in filhos {
                if fila.len() >= MAX_NOS_DECK {
                    break;
                }
                fila.push_back((c, cs, prof + 1));
            }
        }
    }
    melhor
}

/// Entre os grupos de irmãos (mesma classe ou mesma `section`/`article`) com ≥ 3 membros, o de
/// mais membros que tenha um escondido ou o contêiner marcado com `data-mdv-deck`.
fn melhor_grupo<'a>(el: ElementRef<'a>, filhos: &[(ElementRef<'a>, Style)]) -> Option<Vec<ElementRef<'a>>> {
    let marcado = el.value().attr("data-mdv-deck").is_some();
    let mut grupos: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, (c, _)) in filhos.iter().enumerate() {
        let v = c.value();
        let mut classes: Vec<&str> = v.classes().collect();
        classes.sort_unstable();
        classes.dedup();
        for cl in classes {
            grupos.entry(format!("c:{cl}")).or_default().push(i);
        }
        if matches!(v.name(), "section" | "article") {
            grupos.entry(format!("t:{}", v.name())).or_default().push(i);
        }
    }
    let mut escolhido: Option<&Vec<usize>> = None;
    for membros in grupos.values() {
        if membros.len() < 3 {
            continue;
        }
        let escondido = membros
            .iter()
            .any(|&i| filhos.get(i).is_some_and(|(_, s)| s.hidden || s.faded));
        // Classe "slide…" repetida (≥ 3 irmãos) já indica apresentação, mesmo sem nada escondido.
        let tipo_slide = grupos_slide(&grupos, membros);
        if !(escondido || marcado || tipo_slide) {
            continue;
        }
        if escolhido.is_none_or(|e| membros.len() > e.len()) {
            escolhido = Some(membros);
        }
    }
    let membros = escolhido?;
    Some(
        membros
            .iter()
            .filter_map(|&i| filhos.get(i).map(|(c, _)| *c))
            .collect(),
    )
}

/// O grupo `membros` é o de uma classe cujo nome contém "slide" (ex.: `slide`, `slide-content`)?
fn grupos_slide(grupos: &BTreeMap<String, Vec<usize>>, membros: &[usize]) -> bool {
    grupos
        .iter()
        .any(|(k, v)| k.starts_with("c:") && k.to_lowercase().contains("slide") && v.as_slice() == membros)
}
