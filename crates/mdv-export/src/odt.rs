//! Escritor ODT (OpenDocument Text dentro de um ZIP). O `mimetype` vai primeiro e sem compressão,
//! como o formato exige (o `ZipWriter` só grava "stored").

use super::images::to_png;
use super::model::{Block, Document, Inline, ListItem, Run};
use super::xml::escape;
use super::zipw::ZipWriter;
use super::{Budget, Exporter, Output};
use pulldown_cmark::Alignment;

const MIMETYPE: &str = "application/vnd.oasis.opendocument.text";
/// Centímetros por pixel a 96 dpi.
const CM_PER_PX: f64 = 0.026_458_3;
const MAX_IMAGE_W_CM: f64 = 17.0;
const MAX_IMAGE_H_CM: f64 = 22.0;
/// Níveis de lista descritos no estilo.
const LIST_LEVELS: usize = 8;

pub struct OdtExporter;

impl Exporter for OdtExporter {
    fn write(&self, doc: &Document, budget: &dyn Budget) -> Result<Output, String> {
        let mut w = Writer::new(budget);
        let mut body = String::new();
        w.blocks(&doc.blocks, false, &mut body)?;
        let bytes = w.package(doc, &body)?;
        Ok(Output { bytes, warnings: Vec::new() })
    }
}

struct Writer<'a> {
    budget: &'a dyn Budget,
    pictures: Vec<(String, Vec<u8>)>,
    tables: usize,
}

fn is_web_link(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")
}

/// Texto → XML do ODT: espaços repetidos e tabs não podem ser colapsados pelo leitor.
fn text_xml(s: &str) -> String {
    let mut out = String::new();
    let mut spaces = 0usize;
    let flush = |out: &mut String, spaces: &mut usize| {
        match *spaces {
            0 => {}
            1 => out.push(' '),
            n => out.push_str(&format!(" <text:s text:c=\"{}\"/>", n - 1)),
        }
        *spaces = 0;
    };
    for c in s.chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        flush(&mut out, &mut spaces);
        if c == '\t' {
            out.push_str("<text:tab/>");
        } else {
            out.push_str(&escape(&c.to_string()));
        }
    }
    flush(&mut out, &mut spaces);
    // Espaço inicial também seria colapsado.
    if out.starts_with(' ') {
        out = format!("<text:s/>{}", &out[1..]);
    }
    out
}

fn align_style(a: Alignment) -> &'static str {
    match a {
        Alignment::Center => " text:style-name=\"PC\"",
        Alignment::Right => " text:style-name=\"PR\"",
        _ => "",
    }
}

impl<'a> Writer<'a> {
    fn new(budget: &'a dyn Budget) -> Self {
        Writer { budget, pictures: Vec::new(), tables: 0 }
    }


    fn blocks(&mut self, blocks: &[Block], quote: bool, out: &mut String) -> Result<(), String> {
        for b in blocks {
            self.block(b, quote, out)?;
        }
        Ok(())
    }

    fn block(&mut self, b: &Block, quote: bool, out: &mut String) -> Result<(), String> {
        self.budget.check()?;
        let pstyle = if quote { "Quote" } else { "Body" };
        match b {
            Block::Heading { level, text } => {
                let n = (*level).clamp(1, 6);
                out.push_str(&format!("<text:h text:style-name=\"H{n}\" text:outline-level=\"{n}\">{}</text:h>", Self::inlines(text, false)));
            }
            Block::Paragraph(text) => {
                out.push_str(&format!("<text:p text:style-name=\"{pstyle}\">{}</text:p>", Self::inlines(text, false)));
            }
            Block::List { ordered, start, items } => {
                let style = if *ordered { "LN" } else { "LB" };
                out.push_str(&format!("<text:list text:style-name=\"{style}\">"));
                for (i, item) in items.iter().enumerate() {
                    self.list_item(item, *ordered && i == 0 && *start != 1, *start, quote, out)?;
                }
                out.push_str("</text:list>");
            }
            Block::Quote { title, blocks } => {
                if let Some(t) = title {
                    out.push_str(&format!("<text:p text:style-name=\"Quote\"><text:span text:style-name=\"B\">{}</text:span></text:p>", escape(t)));
                }
                self.blocks(blocks, true, out)?;
            }
            Block::Code { text, .. } => {
                for line in text.split('\n') {
                    if line.is_empty() {
                        out.push_str("<text:p text:style-name=\"Code\"/>");
                    } else {
                        out.push_str(&format!("<text:p text:style-name=\"Code\">{}</text:p>", text_xml(line)));
                    }
                }
            }
            Block::Table { aligns, header, rows } => self.table(aligns, header, rows, out),
            Block::Rule => out.push_str("<text:p text:style-name=\"Rule\"/>"),
            Block::Picture { alt, image } => {
                let png = to_png(image)?;
                let n = self.pictures.len() + 1;
                let name = format!("Pictures/image{n}.png");
                self.pictures.push((name.clone(), png));
                let (mut w, mut h) = (f64::from(image.width()) * CM_PER_PX, f64::from(image.height()) * CM_PER_PX);
                if w > MAX_IMAGE_W_CM {
                    h = h * MAX_IMAGE_W_CM / w;
                    w = MAX_IMAGE_W_CM;
                }
                if h > MAX_IMAGE_H_CM {
                    w = w * MAX_IMAGE_H_CM / h;
                    h = MAX_IMAGE_H_CM;
                }
                out.push_str(&format!(
                    "<text:p text:style-name=\"{pstyle}\"><draw:frame draw:name=\"Imagem{n}\" text:anchor-type=\"as-char\" svg:width=\"{w:.3}cm\" svg:height=\"{h:.3}cm\">\
<draw:image xlink:href=\"{name}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/><svg:title>{}</svg:title></draw:frame></text:p>",
                    escape(alt)
                ));
            }
            Block::Image { alt, .. } => {
                out.push_str(&format!("<text:p text:style-name=\"{pstyle}\">[imagem: {}]</text:p>", escape(alt)));
            }
        }
        Ok(())
    }

    fn list_item(&mut self, item: &ListItem, set_start: bool, start: u64, quote: bool, out: &mut String) -> Result<(), String> {
        if set_start {
            out.push_str(&format!("<text:list-item text:start-value=\"{start}\">"));
        } else {
            out.push_str("<text:list-item>");
        }
        let marker = match item.task {
            Some(true) => Some("[x] "),
            Some(false) => Some("[ ] "),
            None => None,
        };
        let mut wrote_marker = marker.is_none();
        for inner in &item.blocks {
            match (inner, wrote_marker) {
                (Block::Paragraph(text), false) => {
                    let pstyle = if quote { "Quote" } else { "Body" };
                    out.push_str(&format!(
                        "<text:p text:style-name=\"{pstyle}\">{}{}</text:p>",
                        escape(marker.unwrap_or("")),
                        Self::inlines(text, false)
                    ));
                    wrote_marker = true;
                }
                _ => self.block(inner, quote, out)?,
            }
        }
        if !wrote_marker {
            out.push_str(&format!("<text:p text:style-name=\"Body\">{}</text:p>", escape(marker.unwrap_or(""))));
        }
        out.push_str("</text:list-item>");
        Ok(())
    }

    fn table(&mut self, aligns: &[Alignment], header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
        self.tables += 1;
        let cols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).max(1);
        out.push_str(&format!(
            "<table:table table:name=\"Tabela{}\" table:style-name=\"Tbl\"><table:table-column table:number-columns-repeated=\"{cols}\"/>",
            self.tables
        ));
        let row = |cells: &[Vec<Inline>], head: bool, out: &mut String| {
            out.push_str("<table:table-row>");
            for c in 0..cols {
                let empty = Vec::new();
                let cell = cells.get(c).unwrap_or(&empty);
                let a = aligns.get(c).copied().unwrap_or(Alignment::None);
                let style = if head { "HeadCell" } else { "Cell" };
                out.push_str(&format!(
                    "<table:table-cell table:style-name=\"{style}\" office:value-type=\"string\"><text:p{}>{}</text:p></table:table-cell>",
                    align_style(a),
                    Self::inlines(cell, head)
                ));
            }
            out.push_str("</table:table-row>");
        };
        if !header.is_empty() {
            out.push_str("<table:table-header-rows>");
            row(header, true, out);
            out.push_str("</table:table-header-rows>");
        }
        for r in rows {
            row(r, false, out);
        }
        out.push_str("</table:table>");
    }

    fn inlines(items: &[Inline], force_bold: bool) -> String {
        let mut out = String::new();
        for item in items {
            match item {
                Inline::Break => out.push_str("<text:line-break/>"),
                Inline::Run(r) => out.push_str(&Self::run(r, force_bold)),
            }
        }
        out
    }

    fn run(r: &Run, force_bold: bool) -> String {
        let mut xml = text_xml(&r.text);
        for (on, style) in [(r.code, "C"), (r.strike, "S"), (r.italic, "I"), (r.bold || force_bold, "B")] {
            if on {
                xml = format!("<text:span text:style-name=\"{style}\">{xml}</text:span>");
            }
        }
        match r.link.as_deref().filter(|u| is_web_link(u)) {
            Some(url) => format!("<text:a xlink:type=\"simple\" xlink:href=\"{}\">{xml}</text:a>", escape(url)),
            None => xml,
        }
    }

    fn package(&self, doc: &Document, body: &str) -> Result<Vec<u8>, String> {
        let content = format!("{CONTENT_HEAD}{}<office:body><office:text>{body}</office:text></office:body></office:document-content>", automatic_styles());
        let mut manifest = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
<manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/>",
        );
        for (name, _) in &self.pictures {
            manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"image/png\"/>", escape(name)));
        }
        manifest.push_str("</manifest:manifest>");
        let meta = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" office:version=\"1.2\"><office:meta>\
<dc:title>{}</dc:title><meta:generator>mdv</meta:generator></office:meta></office:document-meta>",
            escape(doc.title.as_deref().unwrap_or(""))
        );
        let mut zip = ZipWriter::new();
        zip.add("mimetype", MIMETYPE.as_bytes())?;
        zip.add("META-INF/manifest.xml", manifest.as_bytes())?;
        zip.add("content.xml", content.as_bytes())?;
        zip.add("styles.xml", STYLES.as_bytes())?;
        zip.add("meta.xml", meta.as_bytes())?;
        for (name, data) in &self.pictures {
            zip.add(name, data)?;
        }
        zip.finish()
    }
}

const CONTENT_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" office:version=\"1.2\">";

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.2\">\
<office:automatic-styles><style:page-layout style:name=\"pm1\"><style:page-layout-properties fo:page-width=\"21cm\" fo:page-height=\"29.7cm\" \
fo:margin-top=\"2cm\" fo:margin-bottom=\"2cm\" fo:margin-left=\"2cm\" fo:margin-right=\"2cm\"/></style:page-layout></office:automatic-styles>\
<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\"/></office:master-styles>\
</office:document-styles>";

/// Estilos automáticos do `content.xml`: texto, parágrafos, títulos, listas e tabelas.
fn automatic_styles() -> String {
    let mut s = String::from(
        "<office:font-face-decls><style:font-face style:name=\"Courier New\" svg:font-family=\"'Courier New'\" style:font-family-generic=\"modern\"/></office:font-face-decls><office:automatic-styles>\
<style:style style:name=\"B\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
<style:style style:name=\"I\" style:family=\"text\"><style:text-properties fo:font-style=\"italic\"/></style:style>\
<style:style style:name=\"S\" style:family=\"text\"><style:text-properties style:text-line-through-style=\"solid\"/></style:style>\
<style:style style:name=\"C\" style:family=\"text\"><style:text-properties style:font-name=\"Courier New\" fo:background-color=\"#f2f2f2\"/></style:style>\
<style:style style:name=\"Body\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"0.2cm\"/></style:style>\
<style:style style:name=\"Quote\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-left=\"0.6cm\" fo:margin-bottom=\"0.2cm\" fo:padding-left=\"0.3cm\" fo:border-left=\"1.5pt solid #999999\"/></style:style>\
<style:style style:name=\"Code\" style:family=\"paragraph\"><style:paragraph-properties fo:background-color=\"#f2f2f2\" fo:margin-top=\"0cm\" fo:margin-bottom=\"0cm\"/><style:text-properties style:font-name=\"Courier New\" fo:font-size=\"9pt\"/></style:style>\
<style:style style:name=\"Rule\" style:family=\"paragraph\"><style:paragraph-properties fo:border-bottom=\"0.75pt solid #999999\" fo:margin-bottom=\"0.2cm\"/></style:style>\
<style:style style:name=\"PC\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"center\"/></style:style>\
<style:style style:name=\"PR\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"end\"/></style:style>\
<style:style style:name=\"Tbl\" style:family=\"table\"><style:table-properties style:width=\"17cm\" table:align=\"left\"/></style:style>\
<style:style style:name=\"Cell\" style:family=\"table-cell\"><style:table-cell-properties fo:padding=\"0.1cm\" fo:border=\"0.5pt solid #808080\"/></style:style>\
<style:style style:name=\"HeadCell\" style:family=\"table-cell\"><style:table-cell-properties fo:padding=\"0.1cm\" fo:border=\"0.5pt solid #808080\" fo:background-color=\"#e7e6e6\"/></style:style>",
    );
    for (n, size) in [(1, 20), (2, 17), (3, 14), (4, 12), (5, 11), (6, 11)] {
        s.push_str(&format!(
            "<style:style style:name=\"H{n}\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-top=\"0.4cm\" fo:margin-bottom=\"0.2cm\" fo:keep-with-next=\"always\"/>\
<style:text-properties fo:font-size=\"{size}pt\" fo:font-weight=\"bold\"/></style:style>"
        ));
    }
    for (name, number) in [("LB", false), ("LN", true)] {
        s.push_str(&format!("<text:list-style style:name=\"{name}\">"));
        for level in 1..=LIST_LEVELS {
            let left = 0.9 * level as f64;
            let props = format!(
                "<style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" \
text:list-tab-stop-position=\"{left:.2}cm\" fo:text-indent=\"-0.6cm\" fo:margin-left=\"{left:.2}cm\"/></style:list-level-properties>"
            );
            if number {
                s.push_str(&format!("<text:list-level-style-number text:level=\"{level}\" style:num-suffix=\".\" style:num-format=\"1\">{props}</text:list-level-style-number>"));
            } else {
                s.push_str(&format!("<text:list-level-style-bullet text:level=\"{level}\" text:bullet-char=\"•\">{props}</text:list-level-style-bullet>"));
            }
        }
        s.push_str("</text:list-style>");
    }
    s.push_str("</office:automatic-styles>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn export(md: &str) -> Vec<u8> {
        OdtExporter.write(&parse(md, &crate::Unlimited).unwrap(), &crate::Unlimited).unwrap().bytes
    }

    fn contains(bytes: &[u8], needle: &str) -> bool {
        bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn mimetype_e_o_primeiro_arquivo() {
        let b = export("# T\n");
        // Cabeçalho local: 30 bytes fixos + nome ("mimetype") + conteúdo sem compressão.
        assert_eq!(&b[30..38], b"mimetype");
        assert_eq!(&b[38..38 + MIMETYPE.len()], MIMETYPE.as_bytes());
    }

    #[test]
    fn titulos_listas_e_estilos() {
        let b = export("# T\n\n**n** *i* ~~s~~ `c`\n\n- a\n  - filho\n\n3. x\n");
        assert!(contains(&b, "<text:h text:style-name=\"H1\" text:outline-level=\"1\">"));
        assert!(contains(&b, "text:style-name=\"B\"") && contains(&b, "text:style-name=\"I\""));
        assert!(contains(&b, "text:style-name=\"S\"") && contains(&b, "text:style-name=\"C\""));
        assert!(contains(&b, "<text:list text:style-name=\"LB\">"));
        assert!(contains(&b, "text:start-value=\"3\""));
        assert!(b.windows(11).filter(|w| *w == b"<text:list ").count() >= 3, "lista, sublista e ordenada");
    }

    #[test]
    fn codigo_preserva_espacos_e_tabs() {
        let b = export("```\n    a\tb\n```\n");
        assert!(contains(&b, "<text:s text:c=\"3\"/>") || contains(&b, "<text:s/>"), "recuo preservado");
        assert!(contains(&b, "<text:tab/>"));
    }

    #[test]
    fn tabela_link_e_escape() {
        let b = export("| h |\n|---|\n| [x](https://a.b/?q=1&r=2) |\n\na & b\n");
        assert!(contains(&b, "<table:table-header-rows>"));
        assert!(contains(&b, "xlink:href=\"https://a.b/?q=1&amp;r=2\""));
        assert!(contains(&b, "a &amp; b"));
    }

    #[test]
    fn imagem_vai_para_pictures_e_manifesto() {
        let img = std::sync::Arc::new(image::DynamicImage::ImageRgb8(image::RgbImage::new(10, 5)));
        let doc = Document { title: Some("T".into()), blocks: vec![Block::Picture { alt: "foto".into(), image: img }] };
        let b = OdtExporter.write(&doc, &crate::Unlimited).unwrap().bytes;
        assert!(contains(&b, "Pictures/image1.png"));
        assert!(contains(&b, "manifest:full-path=\"Pictures/image1.png\""));
        assert!(contains(&b, "<dc:title>T</dc:title>"));
    }

    #[test]
    fn xml_gerado_e_bem_formado() {
        use crate::xml::assert_balanced;
        let doc = parse("# T\n\n**a** [l](https://x.y) `c`\n\n- a\n  - b\n\n3. c\n\n> [!NOTE]\n> q\n\n| h |\n|---|\n| v |\n\n```\n  x\n```\n\n---\n", &crate::Unlimited).unwrap();
        let mut w = Writer::new(&crate::Unlimited);
        let mut body = String::new();
        w.blocks(&doc.blocks, false, &mut body).unwrap();
        assert_balanced(&format!("<r>{body}</r>"));
        assert_balanced(&format!("<r>{}</r>", automatic_styles().replace("<office:automatic-styles>", "").replace("</office:automatic-styles>", "")));
        assert_balanced(STYLES);
    }

    #[test]
    fn text_xml_converte_espacos_e_tab() {
        assert_eq!(text_xml("a  b"), "a <text:s text:c=\"1\"/>b");
        assert_eq!(text_xml("a\tb"), "a<text:tab/>b");
        assert_eq!(text_xml(" x"), "<text:s/>x");
        assert_eq!(text_xml("<"), "&lt;");
    }
}
