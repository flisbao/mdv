//! Leitura de páginas HTML no terminal: o arquivo (com teto e charset) passa pelo crate
//! `mdv-html`, que devolve células coloridas; aqui elas viram linhas do ratatui e o `Rendered`.

use std::fs;
use std::io;
use std::path::Path;

use mdv_html::{Budget, Face, Options, Rgb};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::{load, mem, render};

/// Teto do arquivo HTML lido do disco (antes de decodificar).
pub(crate) const MAX_HTML_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Quantos bytes do início são procurados em busca de `charset=`.
const SNIFF_BYTES: usize = 2048;

/// Cinza da régua entre páginas.
const RULE_COLOR: Color = Color::Rgb(110, 118, 129);

/// Mesmos limites de largura do `mdv-html`.
const LARGURA_MIN: usize = 20;
const LARGURA_MAX: usize = 400;

/// Extensões de página HTML (sem diferenciar caixa).
pub(crate) fn is_html(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["html", "htm", "xhtml"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Lê um arquivo HTML regular, recusando o que passa de `MAX_HTML_FILE_BYTES` antes de ler, e
/// decodifica o texto (BOM, `charset=` ou UTF-8).
pub(crate) fn read_html_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    if meta.len() > MAX_HTML_FILE_BYTES {
        let teto = MAX_HTML_FILE_BYTES / (1024 * 1024);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("arquivo HTML grande demais (máximo {teto} MiB)"),
        ));
    }
    mem::check_input(meta.len()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let bytes = load::read_limited_bytes(fs::File::open(path)?, MAX_HTML_FILE_BYTES)?;
    Ok(decode(&bytes))
}

/// Texto do HTML: BOM (UTF-8/UTF-16) manda; senão o `charset=` do início, se for um rótulo
/// conhecido; senão UTF-8 com substituição de bytes inválidos.
pub(crate) fn decode(bytes: &[u8]) -> String {
    if let Some((enc, _)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc.decode(bytes).0.into_owned();
    }
    let enc = charset_da_cabeca(bytes)
        .and_then(|rotulo| encoding_rs::Encoding::for_label(rotulo.as_bytes()))
        // `charset=utf-16` no HTML é ignorado (como nos navegadores): o padrão é UTF-8.
        .filter(|enc| !matches!(enc.name(), "UTF-16LE" | "UTF-16BE"));
    match enc {
        Some(enc) => enc.decode(bytes).0.into_owned(),
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// O rótulo de `charset=` nos primeiros `SNIFF_BYTES` bytes (ASCII, sem diferenciar caixa).
fn charset_da_cabeca(bytes: &[u8]) -> Option<String> {
    const CHAVE: &[u8] = b"charset=";
    let cabeca = bytes.get(..SNIFF_BYTES).unwrap_or(bytes).to_ascii_lowercase();
    let pos = cabeca.windows(CHAVE.len()).position(|w| w == CHAVE)?;
    let depois = cabeca.get(pos + CHAVE.len()..)?;
    let inicio = depois.iter().position(|&b| b != b'"' && b != b'\'' && b != b' ')?;
    let rotulo: Vec<u8> = depois
        .get(inicio..)?
        .iter()
        .copied()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
        .collect();
    String::from_utf8(rotulo).ok()
}

/// Orçamento do `mdv-html` ligado ao limite de memória do processo (`mem`).
struct HtmlBudget;

impl Budget for HtmlBudget {
    fn check(&self) -> Result<(), String> {
        mem::check().map_err(|e| e.to_string())
    }
}

/// Converte o HTML em linhas já quebradas em `width` colunas, com cores, links e títulos.
pub(crate) fn render_html(text: &str, width: usize) -> render::Rendered {
    let opts = Options { width, viewport_px: 0, limits: Default::default() };
    match mdv_html::render(text, &opts, &HtmlBudget) {
        Ok(doc) => montar(&doc, width.clamp(LARGURA_MIN, LARGURA_MAX)),
        Err(e) => render::error_rendered(&e),
    }
}

/// Junta as páginas (régua entre elas) e calcula `headings` e `outline` em linhas absolutas.
fn montar(doc: &mdv_html::Document, largura: usize) -> render::Rendered {
    let mut out = render::Rendered { links: doc.links.clone(), ..Default::default() };
    let mut inicios: Vec<usize> = Vec::with_capacity(doc.pages.len());
    for (i, pagina) in doc.pages.iter().enumerate() {
        if let Some(r) = render::memory_error() {
            return r;
        }
        if i > 0 {
            out.lines.push(regua(largura));
        }
        // A página começa na primeira linha de conteúdo (a régua fica acima, visível).
        inicios.push(out.lines.len());
        for linha in &pagina.lines {
            if let Some(r) = render::memory_error() {
                return r;
            }
            out.lines.push(linha_ratatui(linha));
        }
    }

    let mut com_titulo = vec![false; inicios.len()];
    let mut linhas_titulo = inicios.clone();
    let mut outline = Vec::new();
    for h in &doc.headings {
        if let Some(&inicio) = inicios.get(h.page) {
            if let Some(c) = com_titulo.get_mut(h.page) {
                *c = true;
            }
            let linha = inicio.saturating_add(h.line);
            linhas_titulo.push(linha);
            outline.push(render::OutlineEntry { line: linha, level: h.level, title: h.text.clone() });
        }
    }
    for (p, (inicio, tem)) in inicios.iter().zip(&com_titulo).enumerate() {
        if !tem {
            let title = format!("Página {}", p + 1);
            outline.push(render::OutlineEntry { line: *inicio, level: 1, title });
        }
    }
    outline.sort_by_key(|e| e.line);

    let total = out.lines.len();
    linhas_titulo.retain(|&l| l < total);
    linhas_titulo.sort_unstable();
    linhas_titulo.dedup();
    out.headings = linhas_titulo;
    out.outline = outline;
    out
}

/// Régua horizontal de `largura` colunas entre duas páginas.
fn regua(largura: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(largura), Style::default().fg(RULE_COLOR)))
}

/// Uma linha de células vira uma linha do ratatui: células seguidas de mesma `Face` viram um `Span`.
fn linha_ratatui(linha: &[mdv_html::Cell]) -> Line<'static> {
    let mut grupos: Vec<(Face, String)> = Vec::new();
    for c in linha {
        match grupos.last_mut() {
            Some((face, texto)) if *face == c.face => texto.push(c.ch),
            _ => grupos.push((c.face, c.ch.to_string())),
        }
    }
    let spans: Vec<Span<'static>> = grupos
        .into_iter()
        .map(|(face, texto)| Span::styled(render::sanitize(&texto).into_owned(), estilo(&face)))
        .collect();
    Line::from(spans)
}

/// Estilo do ratatui de uma `Face`; link recebe a marca que `render::link_id` lê de volta.
fn estilo(face: &Face) -> Style {
    let mut st = Style::default();
    if let Some(Rgb(r, g, b)) = face.fg {
        st = st.fg(Color::Rgb(r, g, b));
    }
    if let Some(Rgb(r, g, b)) = face.bg {
        st = st.bg(Color::Rgb(r, g, b));
    }
    let mut m = Modifier::empty();
    if face.bold {
        m |= Modifier::BOLD;
    }
    if face.italic {
        m |= Modifier::ITALIC;
    }
    if face.underline {
        m |= Modifier::UNDERLINED;
    }
    if face.dim {
        m |= Modifier::DIM;
    }
    if face.strike {
        m |= Modifier::CROSSED_OUT;
    }
    st = st.add_modifier(m);
    if let Some(id) = face.link {
        st = st.underline_color(render::link_mark(usize::try_from(id).unwrap_or(usize::MAX)));
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::path::Path;
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;

    fn texto(r: &render::Rendered) -> String {
        r.lines.iter().flat_map(|l| l.spans.iter().map(|s| s.content.as_ref())).collect()
    }

    #[test]
    fn extensoes() {
        for a in ["a.html", "a.htm", "a.HTM", "a.xhtml", "pasta/x.Html"] {
            assert!(is_html(Path::new(a)), "{a}");
        }
        for a in ["a.md", "a.msg", "html", "a.html.md"] {
            assert!(!is_html(Path::new(a)), "{a}");
        }
    }

    #[test]
    fn renderiza_titulo_e_cor() {
        let r = render_html("<h1>Oi</h1><p style=\"color:#d97757\">cor</p>", 40);
        let t = texto(&r);
        assert!(t.contains("Oi") && t.contains("cor"), "{t}");
        assert!(r.lines.iter().flat_map(|l| &l.spans).any(|s| s.style.fg == Some(Color::Rgb(217, 119, 87))));
    }

    #[test]
    fn ansi_leva_a_cor_do_html() {
        let r = render_html("<p style=\"color:red\">x</p>", 40);
        assert!(render::to_ansi(&r.lines, true).contains("38;2;255;0;0"));
    }

    #[test]
    fn link_vira_marca_e_destino() {
        let r = render_html("<p><a href=\"https://x.dev\">x</a></p>", 40);
        assert_eq!(r.links, vec!["https://x.dev".to_string()]);
        assert!(r.lines.iter().flat_map(|l| &l.spans).any(|s| render::link_id(&s.style) == Some(0)));
    }

    #[test]
    fn deck_vira_headings_por_pagina() {
        let html = "<main data-mdv-deck><section><h2>Um</h2><p>a</p></section>\
                    <section><h2>Dois</h2></section><section><p>tres</p></section></main>";
        let r = render_html(html, 40);
        assert!(r.headings.len() >= 3, "{:?}", r.headings);
        assert!(r.headings.windows(2).all(|w| w[0] < w[1]), "{:?}", r.headings);
        assert!(r.outline.len() >= 3, "{}", r.outline.len());
    }

    #[test]
    fn erro_de_aninhamento_vira_erro_renderizado() {
        let r = render_html(&"<div>".repeat(6000), 40);
        assert!(texto(&r).contains("aninhado"), "{}", texto(&r));
    }

    #[test]
    fn nenhuma_linha_passa_da_largura() {
        let html = "<p>um texto bem comprido que precisa quebrar em varias linhas de quarenta colunas</p>";
        let r = render_html(html, 40);
        assert!(!r.lines.is_empty());
        for l in &r.lines {
            let w: usize = l.spans.iter().map(|s| UnicodeWidthStr::width(s.content.as_ref())).sum();
            assert!(w <= 40, "{w}");
        }
    }

    #[test]
    fn ler_arquivo_detecta_charset() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.html");
        fs::write(&p, b"<meta charset=\"iso-8859-1\"><p>caf\xe9</p>").unwrap();
        let t = read_html_file(&p).unwrap();
        assert!(t.contains("café"), "{t}");
    }

    #[test]
    fn arquivo_acima_do_teto_e_recusado_sem_ler() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grande.html");
        File::create(&p).unwrap().set_len(MAX_HTML_FILE_BYTES + 1).unwrap();
        let err = read_html_file(&p).unwrap_err();
        assert!(err.to_string().contains("grande demais"), "{err}");
    }

    #[test]
    fn bom_utf16_manda_sobre_o_resto() {
        let bytes: Vec<u8> = [0xFFu8, 0xFE].into_iter().chain("Oi".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode(&bytes), "Oi");
    }
}
