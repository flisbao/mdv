//! Estilo calculado de cada elemento: cascata, herança e `var()`.
//! Os padrões por tag ficam em `ua.rs`, os atributos legados também; a interpretação de cada
//! propriedade (e os atalhos) fica em `style_apply.rs`.

use crate::color::Color;
use crate::css::{self, Declaration};
use crate::guard::Limits;
use crate::index::Index;
use crate::length::Len;
use crate::model::Rgb;
use crate::selector::Pseudo;
use crate::style_apply;
use crate::ua;
use scraper::ElementRef;
use std::collections::HashMap;
use std::rc::Rc;

/// Máximo de variáveis CSS (`--x`) guardadas por elemento.
const MAX_VARS: usize = 256;
/// Substituições de `var()` por valor de declaração (teto contra expansão exponencial).
const PASSOS_VAR: usize = 1024;
/// Profundidade máxima de variáveis que apontam para variáveis.
const PROFUNDIDADE_VAR: usize = 8;

/// Modo de exibição de um elemento.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    None,
    Block,
    #[default]
    Inline,
    InlineBlock,
    Flex,
    Grid,
    ListItem,
    Table,
    TableRow,
    TableCell,
    Contents,
}

/// Alinhamento horizontal do texto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Distribuição dos itens de um flex na linha principal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    Between,
    Around,
}

/// Alinhamento dos itens de um flex na linha transversal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignItems {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

/// Transformação de caixa do texto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transform {
    #[default]
    None,
    Upper,
    Lower,
    Capitalize,
}

/// Tratamento de espaços e quebras de linha.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Pre,
    PreWrap,
    NoWrap,
}

/// Marcador de item de lista.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Marker {
    Disc,
    Circle,
    Square,
    Decimal,
    #[default]
    None,
}

/// Bordas por lado. `Some(cor)` indica que a borda existe; sem cor explícita usa a cor do texto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Border {
    pub top: Option<Rgb>,
    pub right: Option<Rgb>,
    pub bottom: Option<Rgb>,
    pub left: Option<Rgb>,
}

/// Estilo calculado de um elemento. Os campos herdados são copiados do pai; os demais voltam ao padrão.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    // herdados
    pub fg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub dim: bool,
    pub align: Align,
    pub transform: Transform,
    pub white: WhiteSpace,
    pub list_marker: Marker,
    pub vars: Rc<HashMap<String, String>>,
    // não herdados
    pub display: Display,
    pub bg: Option<Color>,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub margin_auto: bool,
    /// Espaçamento interno em px: topo, direita, base, esquerda.
    pub padding: [f32; 4],
    pub width: Option<Len>,
    pub min_width: Option<Len>,
    pub max_width: Option<Len>,
    pub height: Option<Len>,
    pub min_height: Option<Len>,
    pub border: Border,
    /// `border-radius` em px (o primeiro valor); 0 quando não há ou é em %.
    pub radius: f32,
    pub flex_column: bool,
    pub justify: Justify,
    pub align_items: AlignItems,
    /// `flex-grow`: fatia da sobra do flex de linha que o item recebe (0 = não cresce).
    pub flex_grow: f32,
    /// `flex-basis` em px ou %; `None` quando `auto` (ou sem valor).
    pub flex_basis: Option<Len>,
    pub gap: f32,
    pub grid_cols: usize,
    /// Oculto: `display:none`, `visibility:hidden`, `opacity` zero ou atributo `hidden`.
    pub hidden: bool,
    /// Invisível por `opacity` zero ou `visibility:hidden`; independe de `display`.
    pub faded: bool,
    pub fixed: bool,
    pub content: Option<String>,
}

/// Contexto da cascata: índice de regras e tetos.
pub struct Cascade<'a> {
    pub index: &'a Index,
    pub lim: &'a Limits,
}

/// Estilo da raiz do documento (antes do `html`): tudo no padrão, exibição em bloco.
pub fn root_style() -> Style {
    Style {
        display: Display::Block,
        ..Style::default()
    }
}

/// Calcula o estilo de `el` a partir do estilo do pai.
pub fn compute(el: ElementRef<'_>, parent: &Style, cx: &Cascade<'_>) -> Style {
    let mut s = herda(parent);
    ua::aplica_padrao(el, &mut s);
    ua::aplica_legado(el, &mut s);
    let regras = cx.index.matching(el);
    let embutido = el
        .value()
        .attr("style")
        .map(css::parse_declarations)
        .unwrap_or_default();
    let mut lista: Vec<&Declaration> = regras.iter().flat_map(|r| r.decls.iter()).collect();
    lista.extend(embutido.iter());
    aplica_lista(&mut s, &lista, cx.lim);
    if ua::sempre_oculto(el.value().name()) {
        s.display = Display::None;
        s.hidden = true;
    }
    s
}

/// Calcula o estilo do pseudoelemento `::before` ou `::after`. Só devolve estilo se houver `content`.
pub fn compute_pseudo(
    el: ElementRef<'_>,
    parent_style: &Style,
    which: Pseudo,
    cx: &Cascade<'_>,
) -> Option<Style> {
    let regras = cx.index.matching_pseudo(el, which);
    let lista: Vec<&Declaration> = regras.iter().flat_map(|r| r.decls.iter()).collect();
    let mut s = herda(parent_style);
    aplica_lista(&mut s, &lista, cx.lim);
    s.content.as_ref()?;
    Some(s)
}

/// Copia os campos herdados do pai; os não herdados voltam ao padrão.
fn herda(p: &Style) -> Style {
    Style {
        fg: p.fg,
        bold: p.bold,
        italic: p.italic,
        underline: p.underline,
        strike: p.strike,
        dim: p.dim,
        align: p.align,
        transform: p.transform,
        white: p.white,
        list_marker: p.list_marker,
        vars: Rc::clone(&p.vars),
        display: Display::Inline,
        ..Style::default()
    }
}

/// Aplica as declarações em duas passadas: primeiro as variáveis `--*`, depois as demais com `var()` resolvido.
fn aplica_lista(s: &mut Style, lista: &[&Declaration], lim: &Limits) {
    for d in lista {
        if d.name.starts_with("--")
            && (s.vars.len() < MAX_VARS || s.vars.contains_key(&d.name))
        {
            Rc::make_mut(&mut s.vars).insert(d.name.clone(), d.value.clone());
        }
    }
    for d in lista {
        if d.name.starts_with("--") {
            continue;
        }
        let valor = resolve(&d.value, &s.vars, lim.max_value_len);
        style_apply::apply(s, &d.name, &valor);
    }
}

/// Resolve `var(--x)` e `var(--x, reserva)` no valor. Referência circular, profundidade
/// excessiva, orçamento de passos esgotado ou saída maior que `teto` devolvem texto vazio.
pub fn resolve(valor: &str, vars: &HashMap<String, String>, teto: usize) -> String {
    let mut passos = PASSOS_VAR;
    resolve_em(valor, vars, 0, teto, &mut passos)
}

fn resolve_em(
    v: &str,
    vars: &HashMap<String, String>,
    profundidade: usize,
    teto: usize,
    passos: &mut usize,
) -> String {
    if !v.contains("var(") {
        return v.to_string();
    }
    if profundidade > PROFUNDIDADE_VAR {
        return String::new();
    }
    let mut out = String::new();
    let mut resto = v;
    while let Some(i) = resto.find("var(") {
        out.push_str(resto.get(..i).unwrap_or(""));
        let depois = resto.get(i + 4..).unwrap_or("");
        let Some(fim) = fecha(depois) else {
            out.push_str(resto.get(i..).unwrap_or(""));
            return out;
        };
        let dentro = depois.get(..fim).unwrap_or("");
        let (nome, reserva) = match dentro.split_once(',') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (dentro.trim(), None),
        };
        *passos = passos.saturating_sub(1);
        if *passos == 0 {
            return String::new();
        }
        let troca = match (vars.get(nome), reserva) {
            (Some(x), _) => resolve_em(x, vars, profundidade + 1, teto, passos),
            (None, Some(f)) => resolve_em(f, vars, profundidade + 1, teto, passos),
            (None, None) => String::new(),
        };
        out.push_str(&troca);
        if out.len() > teto {
            return String::new();
        }
        resto = depois.get(fim + 1..).unwrap_or("");
    }
    out.push_str(resto);
    if out.len() > teto {
        String::new()
    } else {
        out
    }
}

/// Posição do `)` que fecha um `(` já consumido. `None` se não fecha.
fn fecha(s: &str) -> Option<usize> {
    let mut prof = 1usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => prof += 1,
            ')' => {
                prof -= 1;
                if prof == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css;
    use crate::index;
    use crate::model::Rgb;
    use scraper::{ElementRef, Html};

    /// Calcula o estilo do primeiro elemento que casa com `seletor`, a partir de `html` e `body`.
    fn estilo_de(html: &str, css_src: &str, seletor: &str) -> Style {
        let doc = Html::parse_document(html);
        let lim = Limits::default();
        let idx = index::build(css::parse_sheet(css_src, 800, &lim));
        let cx = Cascade {
            index: &idx,
            lim: &lim,
        };
        let sel = scraper::Selector::parse(seletor).unwrap();
        let el = doc.select(&sel).next().unwrap();
        let mut cadeia: Vec<ElementRef> = el.ancestors().filter_map(ElementRef::wrap).collect();
        cadeia.reverse();
        let mut pai = root_style();
        for a in cadeia {
            pai = compute(a, &pai, &cx);
        }
        compute(el, &pai, &cx)
    }

    fn cor(s: &Style) -> Option<Rgb> {
        s.fg
    }

    fn fundo(s: &Style) -> Option<Rgb> {
        s.bg.map(|c| c.rgb)
    }

    #[test]
    fn padroes_por_tag() {
        let h1 = estilo_de("<h1>T</h1>", "", "h1");
        assert!(h1.bold);
        assert_eq!(h1.display, Display::Block);
        let a = estilo_de("<a href=\"x\">l</a>", "", "a");
        assert!(a.underline);
        assert_eq!(cor(&a), Some(Rgb(88, 166, 255)));
        let pre = estilo_de("<pre>x</pre>", "", "pre");
        assert_eq!(pre.white, WhiteSpace::Pre);
        let script = estilo_de("<script>x</script>", "", "script");
        assert!(script.hidden);
        assert_eq!(script.display, Display::None);
        let ol = estilo_de("<ol><li>a</li></ol>", "", "ol");
        assert_eq!(ol.list_marker, Marker::Decimal);
        let lista = estilo_de("<ul style=\"list-style:none\"><li>a</li></ul>", "", "ul");
        assert_eq!(lista.list_marker, Marker::None);
    }

    #[test]
    fn cascata_por_especificidade_e_ordem() {
        let s = estilo_de(
            "<p class=\"a\">x</p>",
            "p{color:red} .a{color:blue} p{color:green}",
            "p",
        );
        assert_eq!(cor(&s), Some(Rgb(0, 0, 255)));
    }

    #[test]
    fn style_inline_vence() {
        let s = estilo_de(
            "<p id=\"x\" style=\"color:#fff\">x</p>",
            "#x{color:red}",
            "p",
        );
        assert_eq!(cor(&s), Some(Rgb(255, 255, 255)));
    }

    #[test]
    fn herda_cor_mas_nao_fundo() {
        let s = estilo_de(
            "<div style=\"color:red;background:blue\"><span>a</span></div>",
            "",
            "span",
        );
        assert_eq!(cor(&s), Some(Rgb(255, 0, 0)));
        assert_eq!(s.bg, None);
    }

    #[test]
    fn variaveis_com_fallback_e_aninhadas() {
        let s = estilo_de(
            "<html><body><p>x</p></body></html>",
            ":root{--a:#00f;--b:var(--a)} p{color:var(--b); background:var(--nada, #ff0)}",
            "p",
        );
        assert_eq!(cor(&s), Some(Rgb(0, 0, 255)));
        assert_eq!(fundo(&s), Some(Rgb(255, 255, 0)));
    }

    #[test]
    fn variavel_circular_nao_trava() {
        let s = estilo_de(
            "<p>x</p>",
            "p{--a:var(--b);--b:var(--a);color:var(--a)}",
            "p",
        );
        assert_eq!(cor(&s), None);
    }

    #[test]
    fn variaveis_sao_limitadas_a_256() {
        let lim = Limits {
            max_decls: 1000,
            ..Limits::default()
        };
        let css_src: String = format!(
            ":root{{{}}}",
            (0..300).map(|i| format!("--v{i}:1;")).collect::<String>()
        );
        let idx = index::build(css::parse_sheet(&css_src, 800, &lim));
        let cx = Cascade {
            index: &idx,
            lim: &lim,
        };
        let doc = Html::parse_document("<html></html>");
        let s = compute(doc.root_element(), &root_style(), &cx);
        assert_eq!(s.vars.len(), 256);
    }

    #[test]
    fn atalhos() {
        let m = estilo_de("<div style=\"margin:1rem 2rem 3rem\">x</div>", "", "div");
        assert_eq!(m.margin_top, 16.0);
        assert_eq!(m.margin_bottom, 48.0);
        let p = estilo_de("<div style=\"padding:4px 8px\">x</div>", "", "div");
        assert_eq!(p.padding, [4.0, 8.0, 4.0, 8.0]);
        let a = estilo_de("<div style=\"margin:0 auto\">x</div>", "", "div");
        assert!(a.margin_auto);
    }

    #[test]
    fn background_gradiente_pega_ultima_cor() {
        let s = estilo_de(
            "<div>x</div>",
            "div{background: radial-gradient(ellipse at 50% 55%, #161020 0%, #0d1117 65%)}",
            "div",
        );
        assert_eq!(fundo(&s), Some(Rgb(13, 17, 23)));
    }

    #[test]
    fn background_rgba_guarda_alfa() {
        let s = estilo_de("<div>x</div>", "div{background:rgba(63,185,80,.12)}", "div");
        let c = s.bg.unwrap();
        assert_eq!(c.rgb, Rgb(63, 185, 80));
        assert!((c.alpha - 0.12).abs() < 0.01);
    }

    #[test]
    fn opacity_zero_e_visibility_escondem() {
        let o = estilo_de("<div style=\"opacity:0\">x</div>", "", "div");
        assert!(o.faded);
        let v = estilo_de("<p style=\"visibility:hidden\">x</p>", "", "p");
        assert!(v.faded);
        // Um display posterior não desfaz o escondimento por opacidade.
        let d = estilo_de("<div style=\"opacity:0;display:flex\">x</div>", "", "div");
        assert!(d.faded && !d.hidden);
    }

    #[test]
    fn position_fixed() {
        let s = estilo_de("<div style=\"position:fixed\">x</div>", "", "div");
        assert!(s.fixed);
    }

    #[test]
    fn display_flex_column() {
        let a = estilo_de(
            "<div style=\"display:flex;flex-direction:column\">x</div>",
            "",
            "div",
        );
        assert_eq!(a.display, Display::Flex);
        assert!(a.flex_column);
        let b = estilo_de(
            "<div style=\"flex-direction:column;display:flex\">x</div>",
            "",
            "div",
        );
        assert_eq!(b.display, Display::Flex);
        assert!(b.flex_column);
    }

    #[test]
    fn display_none_depois_block_mostra() {
        // Padrão de apresentação: escondido pela regra base, visível pela classe ativa.
        let css_src = ".s{display:none} .s.on{display:block}";
        let on = estilo_de("<div class=\"s on\">x</div>", css_src, "div");
        assert_eq!(on.display, Display::Block);
        assert!(!on.hidden);
        let off = estilo_de("<div class=\"s\">x</div>", css_src, "div");
        assert_eq!(off.display, Display::None);
        assert!(off.hidden);
    }

    #[test]
    fn ocultos_por_tag_nao_voltam_com_css() {
        let s = estilo_de("<script>x</script>", "script{display:block}", "script");
        assert_eq!(s.display, Display::None);
        assert!(s.hidden);
    }

    #[test]
    fn flex_grow_e_basis() {
        let a = estilo_de("<div style=\"flex:1\">x</div>", "", "div");
        assert_eq!(a.flex_grow, 1.0);
        assert_eq!(a.flex_basis, Some(Len::Percent(0.0)));
        let b = estilo_de("<div style=\"flex:2 1 40px\">x</div>", "", "div");
        assert_eq!(b.flex_grow, 2.0);
        assert_eq!(b.flex_basis, Some(Len::Px(40.0)));
        let c = estilo_de("<div style=\"flex:0 0 160px\">x</div>", "", "div");
        assert_eq!(c.flex_grow, 0.0);
        assert_eq!(c.flex_basis, Some(Len::Px(160.0)));
        let d = estilo_de("<div style=\"flex-grow:3;flex-basis:50%\">x</div>", "", "div");
        assert_eq!(d.flex_grow, 3.0);
        assert_eq!(d.flex_basis, Some(Len::Percent(50.0)));
        let e = estilo_de("<div style=\"flex:none\">x</div>", "", "div");
        assert_eq!(e.flex_grow, 0.0);
        assert_eq!(e.flex_basis, None);
        let f = estilo_de("<div style=\"flex:auto\">x</div>", "", "div");
        assert_eq!(f.flex_grow, 1.0);
        assert_eq!(f.flex_basis, None);
    }

    #[test]
    fn grid_colunas() {
        let a = estilo_de(
            "<div style=\"grid-template-columns: repeat(3, 1fr)\">x</div>",
            "",
            "div",
        );
        assert_eq!(a.grid_cols, 3);
        let b = estilo_de("<div style=\"grid-template-columns: 1fr 2fr\">x</div>", "", "div");
        assert_eq!(b.grid_cols, 2);
        let c = estilo_de(
            "<div style=\"grid-template-columns: 200px 1fr 1fr 1fr\">x</div>",
            "",
            "div",
        );
        assert_eq!(c.grid_cols, 4);
    }

    #[test]
    fn borda_esquerda() {
        let s = estilo_de(
            "<div style=\"border-left:3px solid #d97757\">x</div>",
            "",
            "div",
        );
        assert_eq!(s.border.left, Some(Rgb(217, 119, 87)));
        assert_eq!(s.border.right, None);
    }

    #[test]
    fn borda_none_remove() {
        let s = estilo_de("<blockquote>x</blockquote>", "blockquote{border-left:none}", "blockquote");
        assert_eq!(s.border.left, None);
    }

    #[test]
    fn legado() {
        let td = estilo_de(
            "<table><tr><td bgcolor=\"#ff0000\">x</td></tr></table>",
            "",
            "td",
        );
        assert_eq!(fundo(&td), Some(Rgb(255, 0, 0)));
        let font = estilo_de("<font color=\"blue\">x</font>", "", "font");
        assert_eq!(cor(&font), Some(Rgb(0, 0, 255)));
        let p = estilo_de("<p align=\"center\">x</p>", "", "p");
        assert_eq!(p.align, Align::Center);
        let center = estilo_de("<center>x</center>", "", "center");
        assert_eq!(center.align, Align::Center);
    }

    #[test]
    fn content_em_before() {
        let doc = Html::parse_document("<p class=\"x\">t</p>");
        let lim = Limits::default();
        let idx = index::build(css::parse_sheet(".x::before{content:\"→ \"}", 800, &lim));
        let cx = Cascade {
            index: &idx,
            lim: &lim,
        };
        let sel = scraper::Selector::parse(".x").unwrap();
        let el = doc.select(&sel).next().unwrap();
        let estilo = compute(el, &root_style(), &cx);
        let antes = compute_pseudo(el, &estilo, Pseudo::Before, &cx).unwrap();
        assert_eq!(antes.content.as_deref(), Some("→ "));
        assert!(compute_pseudo(el, &estilo, Pseudo::After, &cx).is_none());
    }
}
