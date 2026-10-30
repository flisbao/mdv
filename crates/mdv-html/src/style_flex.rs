//! Atalho `flex` e `flex-grow`/`flex-basis` de `style_apply`: leitura dos valores, sem o resto da
//! cascata. O encolhimento (`flex-shrink`) é lido e ignorado: o layout sempre encolhe até o mínimo.

use crate::length::{parse_len, Len};
use crate::style::Style;

/// Número sem unidade e finito (o `flex-grow`, ou um item do atalho `flex`).
pub(crate) fn numero(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|n| n.is_finite())
}

/// Atalho `flex`: `none`, `auto`, `N` (basis 0), `N basis`, `N shrink` ou `N shrink basis`. O
/// encolhimento não é usado (o layout sempre encolhe até o mínimo), então é lido e ignorado.
pub(crate) fn atalho(s: &mut Style, low: &str) {
    let t: Vec<&str> = low.split_whitespace().collect();
    let base_zero = Len::Percent(0.0);
    match t.as_slice() {
        ["none"] => {
            s.flex_grow = 0.0;
            s.flex_basis = None;
        }
        ["auto"] => {
            s.flex_grow = 1.0;
            s.flex_basis = None;
        }
        [g] => {
            if let Some(g) = numero(g) {
                s.flex_grow = g.max(0.0);
                s.flex_basis = Some(base_zero);
            }
        }
        [a, b] => {
            if let Some(g) = numero(a) {
                s.flex_grow = g.max(0.0);
                match numero(b) {
                    Some(_) => s.flex_basis = Some(base_zero),
                    None => basis_de(s, b),
                }
            }
        }
        [a, _, c] => {
            if let Some(g) = numero(a) {
                s.flex_grow = g.max(0.0);
                basis_de(s, c);
            }
        }
        _ => {}
    }
}

/// Define `flex-basis` a partir de um token (`auto` zera, comprimento válido grava).
fn basis_de(s: &mut Style, token: &str) {
    match parse_len(token) {
        Some(Len::Auto) => s.flex_basis = None,
        Some(l) => s.flex_basis = Some(l),
        None => {}
    }
}
