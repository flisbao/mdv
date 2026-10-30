//! Estilo padrão por tag (folha de navegador embutida) e atributos HTML legados
//! (`bgcolor`, `<font color>`, `align`, `width`). Ficam abaixo do CSS do autor na cascata.

use crate::color::{self, Color};
use crate::length::{parse_len, Len};
use crate::model::Rgb;
use crate::style::{Align, Border, Display, Marker, Style, WhiteSpace};
use crate::style_apply::CINZA;
use scraper::ElementRef;

const LINK: Rgb = Rgb(88, 166, 255);
const FUNDO_CODIGO: Color = Color {
    rgb: CINZA,
    alpha: 0.2,
};
const AMARELO: Rgb = Rgb(255, 235, 59);

/// Elementos que nunca aparecem, mesmo que o CSS do autor peça `display`.
pub fn sempre_oculto(tag: &str) -> bool {
    matches!(
        tag,
        "head"
            | "script"
            | "style"
            | "title"
            | "meta"
            | "link"
            | "template"
            | "noscript"
            | "iframe"
            | "object"
            | "embed"
            | "area"
            | "datalist"
    )
}

/// Aplica os padrões do navegador para a tag do elemento.
pub fn aplica_padrao(el: ElementRef<'_>, s: &mut Style) {
    let v = el.value();
    let tag = v.name();
    match tag {
        "html" | "body" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
        | "dl" | "dt" | "dd" | "pre" | "blockquote" | "section" | "header" | "footer" | "nav"
        | "main" | "article" | "aside" | "figure" | "figcaption" | "form" | "fieldset"
        | "address" | "hr" | "details" | "summary" | "center" | "legend" => {
            s.display = Display::Block
        }
        "li" => s.display = Display::ListItem,
        "table" => s.display = Display::Table,
        "tr" => s.display = Display::TableRow,
        "td" | "th" => s.display = Display::TableCell,
        t if sempre_oculto(t) => {
            s.display = Display::None;
            s.hidden = true;
        }
        _ => {}
    }
    if v.attr("hidden").is_some() {
        s.display = Display::None;
        s.hidden = true;
    }
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            s.bold = true;
            s.margin_bottom = 12.0;
        }
        "strong" | "b" | "th" | "dt" | "legend" => s.bold = true,
        "dd" => pad_esquerda(s, 32.0),
        "em" | "i" | "cite" | "dfn" | "address" => s.italic = true,
        "u" | "ins" => s.underline = true,
        "s" | "strike" | "del" => s.strike = true,
        "small" | "sub" => s.dim = true,
        "code" | "kbd" | "samp" | "tt" => s.bg = Some(FUNDO_CODIGO),
        "pre" => {
            s.white = WhiteSpace::Pre;
            s.bg = Some(FUNDO_CODIGO);
            s.margin_bottom = 16.0;
        }
        "mark" => {
            s.bg = Some(Color {
                rgb: AMARELO,
                alpha: 1.0,
            });
            s.fg = Some(Rgb(0, 0, 0));
        }
        "blockquote" => {
            s.border.left = Some(CINZA);
            pad_esquerda(s, 16.0);
            s.margin_bottom = 16.0;
        }
        "ul" => {
            s.list_marker = Marker::Disc;
            pad_esquerda(s, 16.0);
            s.margin_bottom = 16.0;
        }
        "ol" => {
            s.list_marker = Marker::Decimal;
            pad_esquerda(s, 16.0);
            s.margin_bottom = 16.0;
        }
        "p" | "dl" | "table" | "figure" | "form" => s.margin_bottom = 16.0,
        "hr" => s.border.top = Some(CINZA),
        "center" => s.align = Align::Center,
        "a" if v.attr("href").is_some() => {
            s.fg = Some(LINK);
            s.underline = true;
        }
        _ => {}
    }
}

/// Marcador de `summary` dentro de `details`: `▸ ` quando fechado, `▾ ` quando `open`. O conteúdo
/// do `details` aparece sempre; o marcador só indica o estado.
pub fn marca_detalhes(el: ElementRef<'_>) -> Option<&'static str> {
    if el.value().name() != "summary" {
        return None;
    }
    let pai = el.parent().and_then(ElementRef::wrap)?;
    if pai.value().name() != "details" {
        return None;
    }
    Some(if pai.value().attr("open").is_some() {
        "▾ "
    } else {
        "▸ "
    })
}

/// Aplica os atributos HTML de apresentação (legados). Vêm antes do CSS do autor.
pub fn aplica_legado(el: ElementRef<'_>, s: &mut Style) {
    let v = el.value();
    let tag = v.name();
    if let Some(c) = v.attr("bgcolor").and_then(color::parse) {
        s.bg = Some(c);
    }
    if tag == "font" {
        if let Some(c) = v.attr("color").and_then(color::parse) {
            s.fg = Some(color::blend(c, None));
        }
    }
    if tag == "body" {
        if let Some(c) = v.attr("text").and_then(color::parse) {
            s.fg = Some(color::blend(c, None));
        }
    }
    if let Some(a) = v.attr("align").map(|x| x.trim().to_ascii_lowercase()) {
        let em_lista = matches!(
            tag,
            "td" | "th" | "p" | "div" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        );
        match a.as_str() {
            "center" => s.align = Align::Center,
            "left" if em_lista => s.align = Align::Left,
            "right" if em_lista => s.align = Align::Right,
            _ => {}
        }
    }
    // `border="1"` (qualquer valor que não seja 0) vira as quatro bordas da tabela, em cinza.
    if tag == "table" && v.attr("border").is_some_and(|b| b.trim() != "0") {
        s.border = Border {
            top: Some(CINZA),
            right: Some(CINZA),
            bottom: Some(CINZA),
            left: Some(CINZA),
        };
    }
    if matches!(tag, "table" | "td" | "div") {
        if let Some(l) = v.attr("width").and_then(medida) {
            s.width = Some(l);
        }
    }
}

/// Medida de atributo: número puro é px; senão vale a sintaxe CSS.
fn medida(t: &str) -> Option<Len> {
    let t = t.trim();
    match t.parse::<f32>() {
        Ok(n) if n.is_finite() => Some(Len::Px(n)),
        _ => parse_len(t),
    }
}

fn pad_esquerda(s: &mut Style, px: f32) {
    let [_, _, _, l] = &mut s.padding;
    *l = px;
}
