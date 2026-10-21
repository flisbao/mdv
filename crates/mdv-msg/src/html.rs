//! Conversão do HTML do corpo do e-mail (Outlook) em Markdown.
//! Tabelas de dados viram GFM; tabelas de layout viram sequências de blocos; cabeçalhos
//! de resposta ("De:"/"From:") viram citações aninhadas. Texto do e-mail é escapado uma
//! vez, no ponto em que é emitido (ver `escape`).

use crate::html_table::tabela;
use crate::{escape, Budget, MAX_HTML_BYTES, MAX_HTML_DEPTH};
use scraper::{ElementRef, Html, Node};

/// Elementos que sempre produzem bloco (o resto é transparente ou inline).
const BLOCOS: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "caption", "center", "dd", "div", "dl",
    "dt", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6",
    "header", "hr", "html", "li", "main", "nav", "ol", "p", "pre", "section", "table", "tbody",
    "td", "tfoot", "th", "thead", "tr", "ul",
];
/// Elementos cujo conteúdo nunca vai para o Markdown.
const IGNORADOS: &[&str] = &[
    "head", "style", "script", "title", "meta", "link", "noscript", "template", "iframe", "object",
    "embed", "svg", "canvas", "video", "audio", "input", "button", "select", "textarea",
];

/// Converte o corpo HTML num Markdown com blocos separados por linha em branco.
pub(crate) fn to_markdown(html: &str, budget: &dyn Budget) -> Result<String, String> {
    if html.len() > MAX_HTML_BYTES {
        return Err(format!(
            "corpo HTML grande demais: {} bytes (máximo {MAX_HTML_BYTES})",
            html.len()
        ));
    }
    budget.check_html(html.len())?;
    let doc = Html::parse_document(html);
    let mut f = Flow::new(budget, false, true);
    f.nodes(&[Kid::Elemento(doc.root_element())], 0, 0, false)?;
    Ok(f.finish().join("\n\n").trim_end().to_string())
}

/// Filho de um elemento: texto ou elemento (comentários e demais tipos de nó são descartados).
#[derive(Clone, Copy)]
pub(crate) enum Kid<'a> {
    Texto(&'a str),
    Elemento(ElementRef<'a>),
}

pub(crate) fn kids(el: ElementRef<'_>) -> Vec<Kid<'_>> {
    el.children()
        .filter_map(|n| match n.value() {
            Node::Text(t) => Some(Kid::Texto(&t.text)),
            Node::Element(_) => ElementRef::wrap(n).map(Kid::Elemento),
            _ => None,
        })
        .collect()
}

/// Estado de uma sequência de conteúdo: o trecho em linha em construção e os blocos prontos.
pub(crate) struct Flow<'a> {
    b: &'a dyn Budget,
    /// Dentro de célula de tabela de dados: quebras viram espaço e `|` é escapado.
    cell: bool,
    buf: String,
    /// `<br>` visto e ainda não emitido (só vira quebra se vier texto depois).
    quebra: bool,
    /// Início de bloco (falso dentro de um trecho inline, como `<b>`).
    inicio: bool,
    blocos: Vec<String>,
}

impl<'a> Flow<'a> {
    pub(crate) fn new(b: &'a dyn Budget, cell: bool, inicio: bool) -> Self {
        Flow { b, cell, buf: String::new(), quebra: false, inicio, blocos: Vec::new() }
    }

    pub(crate) fn finish(mut self) -> Vec<String> {
        self.flush();
        self.blocos
    }

    /// Processa `ks` a partir de `from`. `entrada` indica que `ks[from]` é o próprio cabeçalho
    /// de resposta de uma citação, que não deve ser citado de novo.
    pub(crate) fn nodes(&mut self, ks: &[Kid<'_>], from: usize, depth: usize, entrada: bool) -> Result<(), String> {
        let mut i = from;
        while i < ks.len() {
            self.b.check()?;
            match ks[i] {
                Kid::Texto(t) => self.text(t),
                Kid::Elemento(el) => {
                    if depth < MAX_HTML_DEPTH && !(entrada && i == from) && is_reply_header(el) {
                        return self.quote(ks, i, depth);
                    }
                    if el.value().name() == "hr" && proxima_e_cabecalho(ks, i) {
                        i += 1;
                        continue;
                    }
                    self.element(el, depth)?;
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// Cabeçalho de resposta em `ks[from]`: ele e os irmãos seguintes viram uma citação.
    fn quote(&mut self, ks: &[Kid<'_>], from: usize, depth: usize) -> Result<(), String> {
        self.flush();
        let mut q = Flow::new(self.b, self.cell, true);
        q.nodes(ks, from, depth + 1, true)?;
        let blocos = q.finish();
        if !blocos.is_empty() {
            self.blocos.push(cita(&blocos));
        }
        Ok(())
    }

    pub(crate) fn element(&mut self, el: ElementRef<'_>, depth: usize) -> Result<(), String> {
        if depth > MAX_HTML_DEPTH {
            let t = texto_bruto(el);
            self.text(&t);
            return Ok(());
        }
        let nome = el.value().name();
        if IGNORADOS.contains(&nome) {
            return Ok(());
        }
        match nome {
            "br" => self.quebra = true,
            "img" => {
                let md = imagem(el);
                self.piece(&md);
            }
            "a" => {
                let md = self.link(el, depth)?;
                self.piece(&md);
            }
            "b" | "strong" => self.marcado(el, depth, "**")?,
            "i" | "em" => self.marcado(el, depth, "*")?,
            "s" | "strike" | "del" => self.marcado(el, depth, "~~")?,
            _ if BLOCOS.contains(&nome) => {
                self.flush();
                let blocos = bloco(self.b, el, depth, self.cell)?;
                self.blocos.extend(blocos);
            }
            _ => self.nodes(&kids(el), 0, depth + 1, false)?,
        }
        Ok(())
    }

    /// Negrito/itálico/tachado: a marca fica colada ao texto; espaços das pontas ficam de fora.
    fn marcado(&mut self, el: ElementRef<'_>, depth: usize, marca: &str) -> Result<(), String> {
        let miolo = inline_of(self.b, self.cell, el, depth)?;
        let nucleo = miolo.trim_matches(' ');
        let bruto = texto_bruto(el);
        let ini = bruto.starts_with(char::is_whitespace);
        let fim = bruto.ends_with(char::is_whitespace);
        let mut md = String::new();
        if ini {
            md.push(' ');
        }
        if !nucleo.is_empty() {
            md.push_str(&format!("{marca}{nucleo}{marca}"));
        }
        if fim {
            md.push(' ');
        }
        self.piece(&md);
        Ok(())
    }

    /// Link: `[texto](destino)`, ou autolink `<destino>` quando o texto é o próprio endereço.
    fn link(&self, el: ElementRef<'_>, depth: usize) -> Result<String, String> {
        let miolo = inline_of(self.b, self.cell, el, depth)?;
        let texto = miolo.trim();
        let bruto = colapsa(&texto_bruto(el));
        let bruto = bruto.trim();
        let href = el.attr("href").map(|h| mdv_text::sanitize(h.trim()).into_owned()).unwrap_or_default();
        if href.is_empty() {
            return Ok(miolo);
        }
        let auto = !href.chars().any(|c| c.is_whitespace() || c == '<' || c == '>') && href.contains(':');
        if auto && (texto.is_empty() || href == bruto) {
            return Ok(format!("<{href}>"));
        }
        if texto.is_empty() || href == bruto {
            return Ok(miolo);
        }
        Ok(format!("[{texto}]({})", destino(&href)))
    }

    fn text(&mut self, bruto: &str) {
        let limpo = mdv_text::sanitize(bruto);
        let mut t = colapsa(&limpo);
        let ls = self.inicio_de_linha();
        if ls {
            t = t.trim_start_matches(' ').to_string();
        }
        if t.is_empty() {
            return;
        }
        let md = if self.cell {
            escape::cell(&t)
        } else if ls {
            escape::line_start(&t)
        } else {
            escape::inline(&t)
        };
        self.piece(&md);
    }

    fn inicio_de_linha(&self) -> bool {
        if self.quebra {
            return true;
        }
        if self.buf.is_empty() {
            self.inicio
        } else {
            self.buf.ends_with('\n')
        }
    }

    /// Acrescenta um trecho já escapado, aplicando a quebra pendente e os espaços de junção.
    fn piece(&mut self, md: &str) {
        if self.quebra {
            self.quebra = false;
            if !self.buf.is_empty() {
                let len = self.buf.trim_end_matches(' ').len();
                self.buf.truncate(len);
                self.buf.push_str(if self.cell { " " } else { "\\\n" });
            }
        }
        let md = if self.buf.is_empty() || self.buf.ends_with([' ', '\n']) {
            md.trim_start_matches(' ')
        } else {
            md
        };
        self.buf.push_str(md);
    }

    fn flush(&mut self) {
        let t = self.buf.trim_end_matches(' ');
        if !t.is_empty() {
            self.blocos.push(t.to_string());
        }
        self.buf.clear();
        self.quebra = false;
        self.inicio = true;
    }
}

/// Conteúdo inline de um elemento (sem separar blocos; blocos internos viram espaços).
fn inline_of(b: &dyn Budget, cell: bool, el: ElementRef<'_>, depth: usize) -> Result<String, String> {
    let mut f = Flow::new(b, cell, false);
    f.nodes(&kids(el), 0, depth + 1, false)?;
    Ok(f.finish().join(" "))
}

/// Bloco de um elemento de bloco (títulos, listas, citações, código, tabelas, genéricos).
fn bloco(b: &dyn Budget, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let nome = el.value().name();
    match nome {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let nivel = (nome.as_bytes()[1] - b'0') as usize;
            let t = inline_of(b, false, el, depth)?.replace("\\\n", " ");
            let t = t.trim();
            Ok(if t.is_empty() { vec![] } else { vec![format!("{} {t}", "#".repeat(nivel))] })
        }
        "ul" | "ol" => lista(b, el, depth, cell),
        "blockquote" => {
            let blocos = filho_de_bloco(b, el, depth, cell)?;
            Ok(if blocos.is_empty() { vec![] } else { vec![cita(&blocos)] })
        }
        "pre" => Ok(pre(el)),
        "hr" => Ok(vec!["---".to_string()]),
        "table" => tabela(b, el, depth, cell),
        _ => filho_de_bloco(b, el, depth, cell),
    }
}

fn filho_de_bloco(b: &dyn Budget, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let mut f = Flow::new(b, cell, true);
    f.nodes(&kids(el), 0, depth + 1, false)?;
    Ok(f.finish())
}

/// Lista: cada filho direto é um item; itens vazios somem.
fn lista(b: &dyn Budget, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let ordenada = el.value().name() == "ol";
    let inicio = el.attr("start").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1);
    let mut itens: Vec<String> = Vec::new();
    for filho in el.child_elements() {
        b.check()?;
        let mut f = Flow::new(b, cell, true);
        f.element(filho, depth + 1)?;
        let blocos = f.finish();
        if blocos.is_empty() {
            continue;
        }
        let marca = if ordenada { format!("{}. ", inicio + itens.len()) } else { "- ".to_string() };
        let mut item = String::new();
        for (k, bl) in blocos.iter().enumerate() {
            if k == 0 {
                item.push_str(&marca);
                item.push_str(&recua(bl, false));
            } else {
                item.push_str("\n\n");
                item.push_str(&recua(bl, true));
            }
        }
        itens.push(item);
    }
    Ok(if itens.is_empty() { vec![] } else { vec![itens.join("\n")] })
}

/// Bloco de código: conteúdo cru, cercado por crases mais longas que qualquer sequência interna.
fn pre(el: ElementRef<'_>) -> Vec<String> {
    let bruto = texto_bruto(el).replace('\r', "");
    let corpo = bruto.trim_matches('\n');
    if corpo.trim().is_empty() {
        return vec![];
    }
    let (mut maior, mut atual) = (0, 0);
    for c in corpo.chars() {
        atual = if c == '`' { atual + 1 } else { 0 };
        maior = maior.max(atual);
    }
    let cerca = "`".repeat(maior.max(2) + 1);
    vec![format!("{cerca}\n{corpo}\n{cerca}")]
}

/// Citação: cada linha do conteúdo recebe `> ` (linha vazia vira `>`).
pub(crate) fn cita(blocos: &[String]) -> String {
    blocos
        .join("\n\n")
        .split('\n')
        .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Recuo de 2 espaços nas linhas de um bloco de item de lista (a primeira só se `todas`).
fn recua(s: &str, todas: bool) -> String {
    s.split('\n')
        .enumerate()
        .map(|(i, l)| {
            if (i == 0 && !todas) || l.is_empty() {
                l.to_string()
            } else {
                format!("  {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Cabeçalho de resposta do Outlook: id conhecido, ou div com borda superior e "De:"/"From:".
fn is_reply_header(el: ElementRef<'_>) -> bool {
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

/// Indica se o próximo nó que não é espaço em branco, depois de `ks[i]`, é um cabeçalho de resposta.
fn proxima_e_cabecalho(ks: &[Kid<'_>], i: usize) -> bool {
    for k in &ks[i + 1..] {
        match *k {
            Kid::Texto(t) if t.trim().is_empty() => continue,
            Kid::Texto(_) => return false,
            Kid::Elemento(el) => return is_reply_header(el),
        }
    }
    false
}

/// Texto de um elemento, com `<br>` como quebra. Iterativo (não cresce a pilha com a profundidade).
fn texto_bruto(el: ElementRef<'_>) -> String {
    let mut s = String::new();
    for n in el.descendants() {
        match n.value() {
            Node::Text(t) => s.push_str(&mdv_text::sanitize(t)),
            Node::Element(e) if e.name() == "br" => s.push('\n'),
            _ => {}
        }
    }
    s
}

/// Colapsa espaços em branco (inclui U+00A0) num único espaço.
fn colapsa(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    out
}

/// Imagem: `cid:` vira rótulo (o anexo não entra no Markdown); URL remota vira imagem com destino.
fn imagem(el: ElementRef<'_>) -> String {
    let src = el.attr("src").map(|s| mdv_text::sanitize(s.trim()).into_owned()).unwrap_or_default();
    let alt = colapsa(&mdv_text::sanitize(el.attr("alt").unwrap_or(""))).trim().to_string();
    let cid = src.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("cid:"));
    let nome = if cid { src[4..].trim() } else { "" };
    let texto = if alt.is_empty() { nome } else { alt.as_str() };
    let rotulo = if texto.is_empty() {
        "imagem".to_string()
    } else {
        format!("imagem: {}", escape::inline(texto))
    };
    if cid || src.is_empty() {
        format!("[{rotulo}]")
    } else {
        format!("[{rotulo}]({})", destino(&src))
    }
}

/// Destino de link/imagem: espaços e parênteses viram percent-encoding, para não quebrar a sintaxe.
fn destino(url: &str) -> String {
    let mut s = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            '|' => s.push_str("%7C"),
            '\\' => s.push_str("%5C"),
            c if c.is_whitespace() => s.push_str("%20"),
            '(' | ')' | '<' | '>' => s.push_str(&format!("%{:02X}", c as u32)),
            c => s.push(c),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Unlimited, MAX_HTML_BYTES};

    fn md(h: &str) -> String { to_markdown(h, &Unlimited).unwrap() }

    #[test]
    fn paragrafos_e_enfase() {
        assert_eq!(md("<p>Olá <b>Ana</b>,</p><p>segue <i>o</i> relatório.</p>"), "Olá **Ana**,\n\nsegue *o* relatório.");
    }
    #[test]
    fn tabela_de_dados_vira_gfm() {
        assert_eq!(md("<table><tr><td>Sistema</td><td>GB</td></tr><tr><td>CAM</td><td>115,8</td></tr></table>"),
            "| Sistema | GB |\n| --- | --- |\n| CAM | 115,8 |");
    }
    #[test]
    fn tabela_de_layout_aninhada_vira_paragrafos_e_dados_viram_gfm() {
        let h = "<table><tr><td><p>Prezados,</p><table><tr><td>a</td><td>b</td></tr><tr><td>1</td><td>2</td></tr></table><p>Abs</p></td></tr></table>";
        assert_eq!(md(h), "Prezados,\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\nAbs");
    }
    #[test]
    fn colspan_mantem_o_numero_de_colunas() {
        assert_eq!(md("<table><tr><td colspan=2>x</td></tr><tr><td>a</td><td>b</td></tr></table>"),
            "| x |  |\n| --- | --- |\n| a | b |");
    }
    #[test]
    fn barra_vertical_na_celula_e_escapada() {
        assert_eq!(md("<table><tr><td>a|b</td><td>c</td></tr><tr><td>1</td><td>2</td></tr></table>"),
            "| a\\|b | c |\n| --- | --- |\n| 1 | 2 |");
    }
    #[test]
    fn respostas_encadeadas_aprofundam_a_citacao() {
        let h = r#"<div><p>Resposta 2</p><hr><div id="divRplyFwdMsg"><b>De:</b> Ana<br><b>Enviado:</b> ontem</div><p>Resposta 1</p><div style="border:none;border-top:solid #E1E1E1 1.0pt"><p><b>From:</b> Bia</p></div><p>Original</p></div>"#;
        assert_eq!(md(h), "Resposta 2\n\n> **De:** Ana\\\n> **Enviado:** ontem\n>\n> Resposta 1\n>\n> > **From:** Bia\n> >\n> > Original");
    }
    #[test]
    fn imagem_cid_vira_rotulo() {
        assert_eq!(md(r#"<p><img src="cid:logo.png@01" alt="Logo"></p>"#), "[imagem: Logo]");
    }
    #[test]
    fn estilo_script_e_comentario_condicional_somem() {
        assert_eq!(md("<style>p{}</style><!--[if mso]>x<![endif]--><p>a</p>"), "a");
    }
    #[test]
    fn texto_com_markdown_e_escapado() {
        assert_eq!(md("<p># não é título *nem* ênfase</p>"), "\\# não é título \\*nem\\* ênfase");
    }
    #[test]
    fn links_e_listas() {
        assert_eq!(md("<ul><li>a</li><li><a href=\"https://x.y\">x</a></li></ul>"), "- a\n- [x](https://x.y)");
    }
    #[test]
    fn html_acima_do_teto_e_recusado() {
        let e = to_markdown(&"a".repeat(MAX_HTML_BYTES + 1), &Unlimited).unwrap_err();
        assert!(e.contains("grande demais"), "{e}");
    }
    #[test]
    fn aninhamento_profundo_nao_estoura_a_pilha() {
        let r = md(&("<div>".repeat(5000) + "x"));
        assert!(r.contains('x'), "{r}");
    }
}
