//! Marcadores de listas: nível de aninhamento, número inicial de `ol` e texto do marcador.

use crate::guard::Limits;
use crate::style::Marker;
use scraper::ElementRef;

/// Marcadores de disco por nível de aninhamento (ciclo de três).
const DISCOS: [&str; 3] = ["•", "◦", "▪"];

/// Nível da lista `el` (1 = lista de topo). Sobe no máximo `max_walk_depth` ancestrais.
pub fn nivel(el: ElementRef<'_>, lim: &Limits) -> usize {
    1 + el
        .ancestors()
        .filter_map(ElementRef::wrap)
        .take(lim.max_walk_depth)
        .filter(|a| matches!(a.value().name(), "ul" | "ol"))
        .count()
}

/// Primeiro número de uma `ol` (atributo `start`); 1 nas demais.
pub fn inicio(el: ElementRef<'_>) -> i64 {
    if el.value().name() != "ol" {
        return 1;
    }
    el.value()
        .attr("start")
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(1)
}

/// Texto do marcador do item `n` (sem o espaço depois). `None` quando a lista não tem marcador.
pub fn marca(m: Marker, nivel: usize, n: i64) -> Option<String> {
    match m {
        Marker::Disc => DISCOS
            .get(nivel.saturating_sub(1) % DISCOS.len())
            .map(|s| s.to_string()),
        Marker::Circle => Some("◦".to_string()),
        Marker::Square => Some("▪".to_string()),
        Marker::Decimal => Some(format!("{n}.")),
        Marker::None => None,
    }
}
