//! Conversão do HTML do corpo do e-mail (Outlook) em Markdown.
//! Tabelas de dados viram GFM (em `html_table`); tabelas de layout viram sequências de blocos;
//! cabeçalhos de resposta ("De:"/"From:") viram citações aninhadas, até `MAX_NIVEIS_CITACAO`.
//! Texto do e-mail é escapado uma vez, no ponto em que é emitido (ver `escape`).

use crate::html_resposta::{cabecalho_apos_separador, cabecalho_de, is_reply_header, pular_brancos, proxima_e_cabecalho};
use crate::html_lista::lista;
use crate::html_profundidade;
use crate::html_table::tabela;
use crate::{escape, Budget, MAX_HTML_BYTES, MAX_HTML_DEPTH, MAX_MARKDOWN_BYTES};
use scraper::{ElementRef, Html, Node};
use std::cell::Cell;

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
/// Níveis máximos de citação; respostas mais fundas ficam no último nível.
const MAX_NIVEIS_CITACAO: usize = 20;
const MSG_GRANDE_DEMAIS: &str = "e-mail grande demais para converter";

/// Contexto da conversão: orçamento de trabalho e total de saída produzido até agora.
pub(crate) struct Ctx<'a> {
    b: &'a dyn Budget,
    limite: usize,
    usado: Cell<usize>,
}

impl<'a> Ctx<'a> {
    fn new(b: &'a dyn Budget, limite: usize) -> Self {
        Ctx { b, limite, usado: Cell::new(0) }
    }

    /// Confirma o orçamento e o teto de saída; chamado a cada nó, bloco, item, linha e citação.
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.usado.get() > self.limite {
            return Err(MSG_GRANDE_DEMAIS.to_string());
        }
        self.b.check()
    }

    /// Registra `n` bytes produzidos; o teto é conferido em `check`.
    pub(crate) fn gasta(&self, n: usize) {
        self.usado.set(self.usado.get().saturating_add(n));
    }
}

/// Converte o corpo HTML num Markdown com blocos separados por linha em branco.
pub(crate) fn to_markdown(html: &str, budget: &dyn Budget) -> Result<String, String> {
    to_markdown_com_teto(html, budget, MAX_MARKDOWN_BYTES)
}

fn to_markdown_com_teto(html: &str, budget: &dyn Budget, teto: usize) -> Result<String, String> {
    if html.len() > MAX_HTML_BYTES {
        return Err(format!(
            "corpo HTML grande demais: {} bytes (máximo {MAX_HTML_BYTES})",
            html.len()
        ));
    }
    budget.check_html(html.len())?;
    html_profundidade::confere(html)?;
    let doc = Html::parse_document(html);
    let c = Ctx::new(budget, teto);
    let mut f = Flow::new(&c, false, true);
    f.nodes(&[Kid::Elemento(doc.root_element())], 0, 0)?;
    let md = f.finish().join("\n\n").trim_end().to_string();
    c.check()?;
    Ok(md)
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
    c: &'a Ctx<'a>,
    /// Dentro de célula de tabela de dados: quebras viram espaço e `|` é escapado.
    cell: bool,
    buf: String,
    /// `<br>` visto e ainda não emitido (só vira quebra se vier texto depois).
    quebra: bool,
    /// Início de bloco (falso dentro de um trecho inline, como `<b>`).
    inicio: bool,
    /// Quantas citações envolvem este trecho.
    niveis: usize,
    blocos: Vec<String>,
}

impl<'a> Flow<'a> {
    pub(crate) fn new(c: &'a Ctx<'a>, cell: bool, inicio: bool) -> Self {
        Flow { c, cell, buf: String::new(), quebra: false, inicio, niveis: 0, blocos: Vec::new() }
    }

    pub(crate) fn finish(mut self) -> Vec<String> {
        self.flush();
        self.blocos
    }

    /// Processa `ks` a partir de `from`.
    pub(crate) fn nodes(&mut self, ks: &[Kid<'_>], from: usize, depth: usize) -> Result<(), String> {
        let mut i = from;
        while i < ks.len() {
            self.c.check()?;
            match ks[i] {
                Kid::Texto(t) => self.text(t),
                Kid::Elemento(el) => {
                    if self.niveis < MAX_NIVEIS_CITACAO && depth < MAX_HTML_DEPTH && is_reply_header(el) {
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

    /// Cabeçalho de resposta em `ks[i]`: ele e os irmãos seguintes viram uma citação.
    /// Após `appendonsend`, um `<hr>` e o `divRplyFwdMsg` seguinte abrem um nível só.
    fn quote(&mut self, ks: &[Kid<'_>], i: usize, depth: usize) -> Result<(), String> {
        let Kid::Elemento(el) = ks[i] else { return Ok(()) };
        self.flush();
        let cab = cabecalho_de(el).unwrap_or(el);
        let mut q = Flow::new(self.c, self.cell, true);
        q.niveis = self.niveis + 1;
        let filhos = kids(cab);
        let j0 = pular_brancos(&filhos, 0);
        if matches!(filhos.get(j0), Some(Kid::Elemento(h)) if h.value().name() == "hr") {
            // `<hr>` logo no início do cabeçalho é separador, não conteúdo: os filhos seguem sem ele.
            q.nodes(&filhos, j0 + 1, depth + 1)?;
        } else {
            q.element(cab, depth + 1)?;
        }
        let mut j = i + 1;
        if cab.attr("id") == Some("appendonsend") {
            if let Some((k, prox)) = cabecalho_apos_separador(ks, j) {
                q.element(prox, depth + 1)?;
                j = k;
            }
        }
        q.nodes(ks, j, depth + 1)?;
        let blocos = q.finish();
        if !blocos.is_empty() {
            let s = cita(self.c, &blocos)?;
            self.blocos.push(s);
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
                let md = imagem(el, self.cell);
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
                let blocos = bloco(self.c, el, depth, self.cell)?;
                self.blocos.extend(blocos);
            }
            _ => self.nodes(&kids(el), 0, depth + 1)?,
        }
        Ok(())
    }

    /// Negrito/itálico/tachado: a marca fica colada ao texto; espaços das pontas ficam de fora.
    fn marcado(&mut self, el: ElementRef<'_>, depth: usize, marca: &str) -> Result<(), String> {
        let miolo = inline_of(self.c, self.cell, el, depth)?;
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
        let miolo = inline_of(self.c, self.cell, el, depth)?;
        let texto = miolo.trim();
        let bruto = colapsa(&texto_bruto(el));
        let href = el.attr("href").map(|h| mdv_text::sanitize(h.trim()).into_owned()).unwrap_or_default();
        if href.is_empty() {
            return Ok(miolo);
        }
        if autolink_valido(&href) && (texto.is_empty() || href == bruto.trim()) {
            return Ok(format!("<{href}>"));
        }
        if texto.is_empty() {
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
        if md.starts_with('[') && self.buf.ends_with('!') {
            // `!` seguido de `[` formaria imagem: a exclamação vira entidade, sem o `![` no texto.
            self.buf.pop();
            self.buf.push_str("&#33;");
        }
        self.buf.push_str(md);
    }

    fn flush(&mut self) {
        let t = self.buf.trim_end_matches(' ');
        if !t.is_empty() {
            self.c.gasta(t.len());
            self.blocos.push(t.to_string());
        }
        self.buf.clear();
        self.quebra = false;
        self.inicio = true;
    }
}

/// Conteúdo inline de um elemento (sem separar blocos; blocos internos viram espaços).
pub(crate) fn inline_of(c: &Ctx<'_>, cell: bool, el: ElementRef<'_>, depth: usize) -> Result<String, String> {
    let mut f = Flow::new(c, cell, false);
    f.nodes(&kids(el), 0, depth + 1)?;
    Ok(f.finish().join(" "))
}

/// Bloco de um elemento de bloco (títulos, listas, citações, código, tabelas, genéricos).
fn bloco(c: &Ctx<'_>, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let nome = el.value().name();
    match nome {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let nivel = (nome.as_bytes()[1] - b'0') as usize;
            let t = inline_of(c, cell, el, depth)?.replace("\\\n", " ");
            let t = t.trim();
            if t.is_empty() {
                return Ok(vec![]);
            }
            let s = format!("{} {t}", "#".repeat(nivel));
            c.gasta(s.len());
            Ok(vec![s])
        }
        "ul" | "ol" => lista(c, el, depth, cell),
        "blockquote" => {
            let blocos = filho_de_bloco(c, el, depth, cell)?;
            Ok(if blocos.is_empty() { vec![] } else { vec![cita(c, &blocos)?] })
        }
        "pre" => Ok(pre(c, el, cell)),
        "hr" => Ok(vec!["---".to_string()]),
        "table" => tabela(c, el, depth, cell),
        _ => filho_de_bloco(c, el, depth, cell),
    }
}

pub(crate) fn filho_de_bloco(c: &Ctx<'_>, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let mut f = Flow::new(c, cell, true);
    f.nodes(&kids(el), 0, depth + 1)?;
    Ok(f.finish())
}

/// Bloco de código: conteúdo cru, cercado por crases mais longas que qualquer sequência interna.
/// Dentro de célula de tabela vira texto de célula (sem cerca, que quebraria a linha).
fn pre(c: &Ctx<'_>, el: ElementRef<'_>, cell: bool) -> Vec<String> {
    let bruto = texto_bruto(el).replace('\r', "");
    if cell {
        let t = colapsa(&bruto);
        let t = t.trim();
        return if t.is_empty() { vec![] } else { vec![escape::cell(t)] };
    }
    let corpo = bruto.trim_matches('\n');
    if corpo.trim().is_empty() {
        return vec![];
    }
    let (mut maior, mut atual) = (0, 0);
    for ch in corpo.chars() {
        atual = if ch == '`' { atual + 1 } else { 0 };
        maior = maior.max(atual);
    }
    let cerca = "`".repeat(maior.max(2) + 1);
    let s = format!("{cerca}\n{corpo}\n{cerca}");
    c.gasta(s.len());
    vec![s]
}

/// Citação: cada linha do conteúdo recebe `> ` (linha vazia vira `>`).
pub(crate) fn cita(c: &Ctx<'_>, blocos: &[String]) -> Result<String, String> {
    c.check()?;
    let s = blocos
        .join("\n\n")
        .split('\n')
        .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
        .collect::<Vec<_>>()
        .join("\n");
    c.gasta(s.len());
    Ok(s)
}

/// Texto de um elemento, com `<br>` como quebra. Iterativo (não cresce a pilha com a profundidade).
pub(crate) fn texto_bruto(el: ElementRef<'_>) -> String {
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
pub(crate) fn colapsa(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    out
}

/// Autolink `<..>` só com esquema válido (`[A-Za-z][A-Za-z0-9+.-]{1,31}:`) e sem `|`, `\`, `<`, `>` ou espaço.
pub(crate) fn autolink_valido(h: &str) -> bool {
    if h.chars().any(|c| c.is_whitespace() || matches!(c, '|' | '\\' | '<' | '>')) {
        return false;
    }
    let Some((esq, _)) = h.split_once(':') else { return false };
    let mut cs = esq.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic())
        && (2..=32).contains(&esq.len())
        && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// Imagem: `cid:` vira rótulo (o anexo não entra no Markdown); URL remota vira imagem com destino.
fn imagem(el: ElementRef<'_>, cell: bool) -> String {
    let src = el.attr("src").map(|s| mdv_text::sanitize(s.trim()).into_owned()).unwrap_or_default();
    let alt = colapsa(&mdv_text::sanitize(el.attr("alt").unwrap_or(""))).trim().to_string();
    let cid = src.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("cid:"));
    let nome = if cid { src[4..].trim() } else { "" };
    let texto = if alt.is_empty() { nome } else { alt.as_str() };
    let rotulo = if texto.is_empty() {
        "imagem".to_string()
    } else if cell {
        format!("imagem: {}", escape::cell(texto))
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
pub(crate) fn destino(url: &str) -> String {
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
#[path = "html_tests.rs"]
mod tests;
