//! Gráficos e mídia em texto: `<svg>`, `<canvas>`, `<video>` e `<audio>` viram marcadores que
//! preservam o texto alternativo; `<progress>` e `<meter>` viram barras de células coloridas.

use crate::color;
use crate::ctx::Ctx;
use crate::inline::{self, Item};
use crate::length::{px_to_cols, Len};
use crate::model::{Cell, Face, Rgb};
use crate::style::Style;
use scraper::ElementRef;

/// Máximo de caracteres do texto de um gráfico `<svg>` no marcador.
const MAX_GRAFICO: usize = 80;
/// Largura padrão, em células, de uma barra sem `width` no CSS.
const BARRA_PADRAO: usize = 20;
/// Largura mínima, em células, de uma barra com `width` no CSS.
const BARRA_MIN: usize = 6;
/// Largura máxima, em células, de uma barra com `width` no CSS.
const BARRA_MAX: usize = 200;
/// Cor padrão do preenchimento de `<progress>` (verde).
const VERDE: Rgb = Rgb(63, 185, 80);
/// Cor padrão do preenchimento de `<meter>` (amarelo).
const AMARELO: Rgb = Rgb(210, 153, 34);

/// Texto de um `<svg>`: o conteúdo de `<title>`, `<desc>` e `<text>` (com `tspan` dentro), em
/// ordem de documento, separado por espaço e cortado em `MAX_GRAFICO` caracteres.
pub fn texto_svg(el: ElementRef<'_>, cx: &Ctx<'_>) -> Result<String, String> {
    let mut partes: Vec<String> = Vec::new();
    let mut total = 0usize;
    let mut pilha = inline::filhos(el);
    pilha.reverse();
    while let Some(item) = pilha.pop() {
        cx.tick()?;
        if total >= MAX_GRAFICO {
            break;
        }
        let Item::Elem(e) = item else { continue };
        match e.value().name() {
            "text" | "title" | "desc" => {
                let t = inline::texto_visivel(e, cx)?;
                if !t.is_empty() {
                    total = total.saturating_add(t.chars().count() + 1);
                    partes.push(t);
                }
            }
            _ => {
                let mut sub = inline::filhos(e);
                sub.reverse();
                pilha.extend(sub);
            }
        }
    }
    Ok(partes
        .join(" ")
        .chars()
        .take(MAX_GRAFICO)
        .collect::<String>()
        .trim_end()
        .to_string())
}

/// Barra de `<progress>` ou `<meter>`: `█` até a fração do valor e `░` no resto, sem texto.
/// A largura vem do `width` do CSS (px ou %), ou 20 colunas, sempre limitada a `disponivel`.
pub fn barra(
    el: ElementRef<'_>,
    cs: &Style,
    pai: &Style,
    bg: Option<Rgb>,
    link: Option<u32>,
    disponivel: usize,
) -> Vec<Cell> {
    let limite = disponivel.clamp(1, BARRA_MAX);
    let pedida = match cs.width {
        Some(Len::Px(px)) => px_to_cols(px),
        Some(Len::Percent(p)) => ((disponivel as f32) * p / 100.0).round().max(0.0) as usize,
        _ => BARRA_PADRAO,
    };
    let n = pedida.clamp(BARRA_MIN, BARRA_MAX).min(limite);
    let cheio = ((fracao(el) * n as f32).round().max(0.0) as usize).min(n);
    // Cor própria do elemento só quando difere da herdada; senão, a cor padrão do tipo.
    let padrao = if el.value().name() == "meter" { AMARELO } else { VERDE };
    let cor = match cs.fg {
        Some(f) if Some(f) != pai.fg => f,
        _ => padrao,
    };
    let preenchido = Face {
        fg: color::fix_contrast(Some(cor), bg),
        bg,
        link,
        ..Face::default()
    };
    let vazio = Face {
        dim: true,
        ..preenchido
    };
    (0..n)
        .map(|i| {
            if i < cheio {
                inline::celula('█', preenchido)
            } else {
                inline::celula('░', vazio)
            }
        })
        .collect()
}

/// Fração preenchida de `<progress>` (min 0, max padrão 1) ou `<meter>` (min e max), entre 0 e 1.
/// Valor ausente ou inválido vale 0.
fn fracao(el: ElementRef<'_>) -> f32 {
    let v = el.value();
    let minimo = if v.name() == "meter" {
        num(v.attr("min")).unwrap_or(0.0)
    } else {
        0.0
    };
    let maximo = num(v.attr("max"))
        .filter(|m| *m > minimo)
        .unwrap_or(minimo + 1.0);
    match num(v.attr("value")) {
        Some(valor) => ((valor - minimo) / (maximo - minimo)).clamp(0.0, 1.0),
        None => 0.0,
    }
}

fn num(s: Option<&str>) -> Option<f32> {
    s?.trim().parse::<f32>().ok().filter(|x| x.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fracao_de_progress_e_meter() {
        let doc = scraper::Html::parse_fragment(
            "<progress value=\"78\" max=\"100\"></progress>\
             <progress value=\"3\"></progress>\
             <progress max=\"100\"></progress>\
             <meter min=\"100\" max=\"200\" value=\"150\"></meter>",
        );
        let sel = scraper::Selector::parse("progress, meter").unwrap();
        let fs: Vec<f32> = doc.select(&sel).map(fracao).collect();
        assert_eq!(fs.len(), 4);
        assert!((fs[0] - 0.78).abs() < 1e-6);
        assert_eq!(fs[1], 1.0);
        assert_eq!(fs[2], 0.0);
        assert!((fs[3] - 0.5).abs() < 1e-6);
    }
}
