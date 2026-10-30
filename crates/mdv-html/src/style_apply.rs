//! Interpretação de cada propriedade CSS sobre o `Style` (`apply`), resolução de `var()` e atalhos.
//! Valor que não se reconhece é ignorado: a propriedade fica como estava.

use crate::color::{self, Color};
use crate::length::{parse_len, Len};
use crate::model::Rgb;
use crate::style::{
    Align, AlignItems, Border, Display, Justify, Marker, Style, Transform, WhiteSpace,
};

/// Cor de borda e de marcador quando a regra não diz cor e o texto não tem cor.
pub const CINZA: Rgb = Rgb(110, 118, 129);

/// Aplica uma declaração (nome em minúsculas, valor já com `var()` resolvido).
pub fn apply(s: &mut Style, nome: &str, valor: &str) {
    if nome.starts_with("--") {
        return;
    }
    let v = valor.trim();
    let low = v.to_ascii_lowercase();
    match nome {
        "color" => {
            if let Some(c) = color::parse(v).filter(|c| c.alpha > 0.0) {
                s.fg = Some(color::blend(c, None));
            }
        }
        "background" | "background-color" => fundo(s, v),
        "font-weight" => match low.as_str() {
            "bold" | "bolder" => s.bold = true,
            "normal" | "lighter" => s.bold = false,
            n => {
                if let Ok(x) = n.parse::<u32>() {
                    s.bold = x >= 600;
                }
            }
        },
        "font-style" => {
            if low.starts_with("italic") || low.starts_with("oblique") {
                s.italic = true;
            } else if low == "normal" {
                s.italic = false;
            }
        }
        "text-decoration" | "text-decoration-line" => {
            let tokens: Vec<&str> = low.split_whitespace().collect();
            s.underline = tokens.contains(&"underline");
            s.strike = tokens.contains(&"line-through");
        }
        "text-align" => match low.as_str() {
            "center" => s.align = Align::Center,
            "right" | "end" => s.align = Align::Right,
            "left" | "start" | "justify" => s.align = Align::Left,
            _ => {}
        },
        "text-transform" => match low.as_str() {
            "uppercase" => s.transform = Transform::Upper,
            "lowercase" => s.transform = Transform::Lower,
            "capitalize" => s.transform = Transform::Capitalize,
            "none" => s.transform = Transform::None,
            _ => {}
        },
        "white-space" => match low.as_str() {
            "normal" => s.white = WhiteSpace::Normal,
            "pre" => s.white = WhiteSpace::Pre,
            "pre-wrap" | "pre-line" => s.white = WhiteSpace::PreWrap,
            "nowrap" => s.white = WhiteSpace::NoWrap,
            _ => {}
        },
        "display" => {
            if low == "none" {
                s.display = Display::None;
                s.hidden = true;
            } else if let Some(d) = display_de(&low) {
                s.display = d;
                s.hidden = false;
            }
        }
        "flex-direction" => s.flex_column = low.starts_with("column"),
        "justify-content" => match low.as_str() {
            "flex-start" | "start" | "normal" | "left" => s.justify = Justify::Start,
            "center" => s.justify = Justify::Center,
            "flex-end" | "end" | "right" => s.justify = Justify::End,
            "space-between" => s.justify = Justify::Between,
            "space-around" | "space-evenly" => s.justify = Justify::Around,
            _ => {}
        },
        "align-items" => match low.as_str() {
            "flex-start" | "start" => s.align_items = AlignItems::Start,
            "center" => s.align_items = AlignItems::Center,
            "flex-end" | "end" => s.align_items = AlignItems::End,
            "stretch" | "normal" => s.align_items = AlignItems::Stretch,
            _ => {}
        },
        "flex" => crate::style_flex::atalho(s, &low),
        "flex-grow" => {
            if let Some(n) = crate::style_flex::numero(&low) {
                s.flex_grow = n.max(0.0);
            }
        }
        "flex-basis" => match parse_len(v) {
            Some(Len::Auto) => s.flex_basis = None,
            Some(l) => s.flex_basis = Some(l),
            None => {}
        },
        "gap" | "column-gap" | "row-gap" => {
            if let Some(px) = maior_px(v) {
                s.gap = px;
            }
        }
        "grid-template-columns" => {
            if let Some(n) = conta_colunas(&low) {
                s.grid_cols = n;
            }
        }
        "margin" => margem_atalho(s, &low),
        "margin-top" => {
            if let Some(px) = parse_len(&low).and_then(px_de) {
                s.margin_top = px;
            }
        }
        "margin-bottom" => {
            if let Some(px) = parse_len(&low).and_then(px_de) {
                s.margin_bottom = px;
            }
        }
        "margin-left" | "margin-right" => {
            if low == "auto" {
                s.margin_auto = true;
            }
        }
        "padding" => {
            let ps: Option<Vec<f32>> = tokens(&low)
                .iter()
                .map(|t| parse_len(t).and_then(px_de))
                .collect();
            if let Some(arr) = ps.and_then(|p| expande(&p)) {
                s.padding = arr;
            }
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            if let Some(px) = parse_len(&low).and_then(px_de) {
                let [t, r, b, l] = &mut s.padding;
                match nome {
                    "padding-top" => *t = px,
                    "padding-right" => *r = px,
                    "padding-bottom" => *b = px,
                    _ => *l = px,
                }
            }
        }
        "width" => {
            if let Some(l) = parse_len(v) {
                s.width = Some(l);
            }
        }
        "min-width" => {
            if let Some(l) = parse_len(v) {
                s.min_width = Some(l);
            }
        }
        "height" => {
            if let Some(l) = parse_len(v) {
                s.height = Some(l);
            }
        }
        "min-height" => {
            if let Some(l) = parse_len(v) {
                s.min_height = Some(l);
            }
        }
        "max-width" => {
            if low == "none" {
                s.max_width = None;
            } else if let Some(l) = parse_len(v) {
                s.max_width = Some(l);
            }
        }
        "border" => {
            if let Some(b) = borda(&low, s.fg) {
                s.border = Border {
                    top: b,
                    right: b,
                    bottom: b,
                    left: b,
                };
            }
        }
        "border-top" => {
            if let Some(b) = borda(&low, s.fg) {
                s.border.top = b;
            }
        }
        "border-right" => {
            if let Some(b) = borda(&low, s.fg) {
                s.border.right = b;
            }
        }
        "border-bottom" => {
            if let Some(b) = borda(&low, s.fg) {
                s.border.bottom = b;
            }
        }
        "border-left" => {
            if let Some(b) = borda(&low, s.fg) {
                s.border.left = b;
            }
        }
        "border-radius" => {
            s.radius = match tokens(&low).first().and_then(|t| parse_len(t)) {
                Some(Len::Px(px)) => px.max(0.0),
                _ => 0.0,
            };
        }
        "border-color" => {
            if let Some(c) = color::parse(v).map(|c| color::blend(c, None)) {
                let b = &mut s.border;
                for lado in [&mut b.top, &mut b.right, &mut b.bottom, &mut b.left] {
                    if lado.is_some() {
                        *lado = Some(c);
                    }
                }
            }
        }
        "opacity" => {
            if let Ok(x) = low.parse::<f32>() {
                if x.is_finite() {
                    s.faded = x <= 0.01;
                }
            }
        }
        "visibility" => match low.as_str() {
            "hidden" | "collapse" => s.faded = true,
            "visible" => s.faded = false,
            _ => {}
        },
        "position" => s.fixed = low == "fixed",
        "content" => {
            if low == "none" || low == "normal" {
                s.content = None;
            } else if let Some(t) = texto_de(v) {
                s.content = Some(t);
            }
        }
        "list-style" | "list-style-type" => {
            for t in low.split_whitespace() {
                if let Some(m) = marcador(t) {
                    s.list_marker = m;
                }
            }
        }
        _ => {}
    }
}

/// Separa por espaços e vírgulas que estão fora de parênteses.
fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut atual = String::new();
    let mut prof = 0usize;
    for c in s.chars() {
        match c {
            '(' => prof += 1,
            ')' => prof = prof.saturating_sub(1),
            _ => {}
        }
        if prof == 0 && (c.is_whitespace() || c == ',') {
            if !atual.is_empty() {
                out.push(std::mem::take(&mut atual));
            }
        } else {
            atual.push(c);
        }
    }
    if !atual.is_empty() {
        out.push(atual);
    }
    out
}

fn display_de(low: &str) -> Option<Display> {
    match low {
        "block" | "flow-root" | "table-row-group" | "table-header-group" | "table-footer-group"
        | "table-caption" => Some(Display::Block),
        "inline" => Some(Display::Inline),
        "inline-block" => Some(Display::InlineBlock),
        "contents" => Some(Display::Contents),
        "flex" | "inline-flex" => Some(Display::Flex),
        "grid" | "inline-grid" => Some(Display::Grid),
        "list-item" => Some(Display::ListItem),
        "table" | "inline-table" => Some(Display::Table),
        "table-row" => Some(Display::TableRow),
        "table-cell" => Some(Display::TableCell),
        _ => None,
    }
}

fn marcador(t: &str) -> Option<Marker> {
    match t {
        "disc" => Some(Marker::Disc),
        "circle" => Some(Marker::Circle),
        "square" => Some(Marker::Square),
        "decimal" | "decimal-leading-zero" => Some(Marker::Decimal),
        "none" => Some(Marker::None),
        _ => None,
    }
}

/// Fundo: a última cor de um gradiente; senão a primeira cor da declaração; `none` limpa.
fn fundo(s: &mut Style, v: &str) {
    let low = v.to_ascii_lowercase();
    if low.contains("gradient(") {
        let ini = low.find('(').map_or(0, |i| i + 1);
        let fim = low.rfind(')').unwrap_or(low.len());
        let cor = low
            .get(ini..fim)
            .map(tokens)
            .unwrap_or_default()
            .iter()
            .filter_map(|t| color::parse(t))
            .next_back();
        if let Some(c) = cor {
            s.bg = Some(c);
        }
        return;
    }
    let toks = tokens(&low);
    if let Some(c) = toks.iter().find_map(|t| color::parse(t)) {
        s.bg = Some(c);
    } else if toks.iter().any(|t| t == "none") {
        s.bg = None;
    }
}

/// Lê uma borda. `None`: não entendeu. `Some(None)`: sem borda. `Some(Some(cor))`: borda com a cor.
fn borda(low: &str, fg: Option<Rgb>) -> Option<Option<Rgb>> {
    let toks = tokens(low);
    if toks.is_empty() {
        return None;
    }
    let mut largura: Option<f32> = None;
    let mut estilo: Option<&str> = None;
    let mut cor: Option<Color> = None;
    for t in &toks {
        let t = t.as_str();
        if let Some(Len::Px(px)) = parse_len(t) {
            largura = Some(px);
        } else if matches!(t, "thin" | "medium" | "thick") {
            largura = Some(1.0);
        } else if matches!(
            t,
            "none" | "hidden" | "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge"
                | "inset" | "outset"
        ) {
            estilo = Some(t);
        } else {
            cor = Some(color::parse(t)?);
        }
    }
    if largura == Some(0.0) || matches!(estilo, Some("none" | "hidden")) {
        return Some(None);
    }
    let c = cor
        .map(|c| color::blend(c, None))
        .or(fg)
        .unwrap_or(CINZA);
    Some(Some(c))
}

/// Maior valor em px entre os tokens (ignora porcentagens).
fn maior_px(v: &str) -> Option<f32> {
    tokens(v)
        .iter()
        .filter_map(|t| match parse_len(t) {
            Some(Len::Px(px)) => Some(px),
            _ => None,
        })
        .reduce(f32::max)
}

/// Quantidade de colunas de `grid-template-columns`: `repeat(N, …)` conta N, `auto-fit`/`auto-fill` contam 3.
fn conta_colunas(v: &str) -> Option<usize> {
    if v == "none" {
        return Some(0);
    }
    let mut total = 0usize;
    for t in tokens(v) {
        if t.starts_with('[') {
            continue;
        }
        let repeticao = t
            .strip_prefix("repeat(")
            .and_then(|r| r.strip_suffix(')'));
        if let Some(inner) = repeticao {
            let n = inner.split(',').next()?.trim();
            let k = match n {
                "auto-fit" | "auto-fill" => 3,
                _ => n.parse::<usize>().ok()?,
            };
            total = total.saturating_add(k);
        } else {
            total = total.saturating_add(1);
        }
    }
    if total == 0 {
        None
    } else {
        Some(total)
    }
}

/// Expande 1 a 4 valores de atalho (topo, direita, base, esquerda) na ordem do CSS.
fn expande<T: Copy>(p: &[T]) -> Option<[T; 4]> {
    match *p {
        [a] => Some([a, a, a, a]),
        [a, b] => Some([a, b, a, b]),
        [a, b, c] => Some([a, b, c, b]),
        [a, b, c, d] => Some([a, b, c, d]),
        _ => None,
    }
}

/// Atalho `margin`: só guarda topo, base e se a margem horizontal é `auto`.
fn margem_atalho(s: &mut Style, low: &str) {
    let partes: Option<Vec<Len>> = tokens(low).iter().map(|t| parse_len(t)).collect();
    let Some(ps) = partes else {
        return;
    };
    let Some([t, r, b, l]) = expande(&ps) else {
        return;
    };
    s.margin_top = px_de(t).unwrap_or(0.0);
    s.margin_bottom = px_de(b).unwrap_or(0.0);
    s.margin_auto = l == Len::Auto || r == Len::Auto;
}

/// Comprimento em px; porcentagem e `auto` não viram px.
fn px_de(l: Len) -> Option<f32> {
    match l {
        Len::Px(px) => Some(px),
        _ => None,
    }
}

/// Texto de `content`: uma ou mais strings entre aspas, com escapes CSS (`\201C`, `\"`). Outras
/// funções (`counter()`, `attr()`, `url()`) não entram: devolvem `None`.
fn texto_de(v: &str) -> Option<String> {
    let mut chars = v.trim().chars().peekable();
    let mut out = String::new();
    let mut partes = 0usize;
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(aspa) = chars.next() else { break };
        if aspa != '"' && aspa != '\'' {
            return None;
        }
        loop {
            let c = chars.next()?;
            if c == aspa {
                break;
            }
            if c == '\\' {
                out.push(escape_css(&mut chars)?);
            } else {
                out.push(c);
            }
        }
        partes += 1;
    }
    (partes > 0).then_some(out)
}

/// Decodifica um escape CSS depois da barra: até seis dígitos hexadecimais (com um espaço opcional
/// no fim) ou o próprio caractere seguinte. Código inválido vira U+FFFD.
fn escape_css(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    let mut hex = String::new();
    while hex.len() < 6 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
        hex.push(chars.next()?);
    }
    if hex.is_empty() {
        return chars.next();
    }
    if chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
    let codigo = u32::from_str_radix(&hex, 16).ok()?;
    Some(char::from_u32(codigo).filter(|c| *c != '\0').unwrap_or('\u{fffd}'))
}
