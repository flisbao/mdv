//! Detecção de cabeçalhos de resposta do Outlook (OWA, desktop e wrappers sem atributos).

use crate::html::Kid;
use scraper::ElementRef;

/// Cabeçalho de resposta: o próprio elemento, ou um wrapper sem atributos (até 3 níveis)
/// cujo único filho elemento é o cabeçalho. Devolve o cabeçalho.
pub(crate) fn cabecalho_de(el: ElementRef<'_>) -> Option<ElementRef<'_>> {
    achar(el, 3)
}

fn achar(el: ElementRef<'_>, niveis: usize) -> Option<ElementRef<'_>> {
    if cabecalho_direto(el) {
        return Some(el);
    }
    if niveis == 0 || el.value().name() != "div" || el.value().attrs().next().is_some() {
        return None;
    }
    if el.children().any(|n| n.value().as_text().is_some_and(|t| !t.trim().is_empty())) {
        return None;
    }
    let mut filhos = el.child_elements();
    let unico = filhos.next()?;
    if filhos.next().is_some() {
        return None;
    }
    achar(unico, niveis - 1)
}

pub(crate) fn is_reply_header(el: ElementRef<'_>) -> bool {
    cabecalho_de(el).is_some()
}

/// Cabeçalho de resposta direto: id conhecido, ou div com borda superior e "De:"/"From:".
fn cabecalho_direto(el: ElementRef<'_>) -> bool {
    if matches!(el.attr("id"), Some("divRplyFwdMsg" | "appendonsend")) {
        return true;
    }
    if el.value().name() != "div" {
        return false;
    }
    let estilo: String = el.attr("style").unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect();
    if !estilo.to_ascii_lowercase().contains("border-top:solid") {
        return false;
    }
    let mut inicio = String::new();
    for pedaco in el.text() {
        inicio.push_str(pedaco);
        if inicio.trim_start().chars().count() >= 8 {
            break;
        }
    }
    let inicio = inicio.trim_start();
    inicio.starts_with("De:") || inicio.starts_with("From:")
}

/// Pula textos só com espaço a partir de `j`.
fn pular_brancos(ks: &[Kid<'_>], mut j: usize) -> usize {
    while matches!(ks.get(j), Some(Kid::Texto(t)) if t.trim().is_empty()) {
        j += 1;
    }
    j
}

/// Após `appendonsend`: pula espaços e um `<hr>`; se vier um cabeçalho, devolve (próximo índice, cabeçalho).
pub(crate) fn cabecalho_apos_separador<'a>(ks: &[Kid<'a>], j: usize) -> Option<(usize, ElementRef<'a>)> {
    let mut j = pular_brancos(ks, j);
    if matches!(ks.get(j), Some(Kid::Elemento(e)) if e.value().name() == "hr") {
        j = pular_brancos(ks, j + 1);
    }
    match ks.get(j) {
        Some(Kid::Elemento(e)) => cabecalho_de(*e).map(|h| (j + 1, h)),
        _ => None,
    }
}

/// Indica se o próximo nó que não é espaço em branco, depois de `ks[i]`, é um cabeçalho de resposta.
pub(crate) fn proxima_e_cabecalho(ks: &[Kid<'_>], i: usize) -> bool {
    let j = pular_brancos(ks, i + 1);
    matches!(ks.get(j), Some(Kid::Elemento(e)) if is_reply_header(*e))
}
