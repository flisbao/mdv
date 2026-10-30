//! Bloco `<pre>`: texto cru com as cores dos spans, uma linha por `\n`, sem quebra por largura.

use crate::ctx::Ctx;
use crate::inline;
use crate::model::{Line, Rgb};
use crate::style::{Align, Style};
use crate::text::{align, blank, truncate};
use scraper::ElementRef;

/// Linhas do `<pre>` na largura `width`: recuo de 1 coluna, linhas mais largas truncadas com `…`,
/// preenchidas com `bg`. Linhas em branco do fim são descartadas.
pub fn pre_lines(
    el: ElementRef<'_>,
    st: &Style,
    width: usize,
    bg: Option<Rgb>,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let celulas = inline::celulas_filhos(el, st, bg, cx)?;
    let mut linhas: Vec<Line> = celulas.split(|c| c.ch == '\n').map(|s| s.to_vec()).collect();
    while linhas.last().is_some_and(|l| l.is_empty()) {
        linhas.pop();
    }
    cx.linhas(linhas.len())?;
    let mut out = Vec::with_capacity(linhas.len());
    for l in linhas {
        let mut x = blank(1, bg);
        x.extend(truncate(&l, width.saturating_sub(2)));
        out.push(align(x, width, Align::Left, bg));
    }
    Ok(out)
}
