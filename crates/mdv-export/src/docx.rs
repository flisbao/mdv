//! Escritor DOCX (WordprocessingML dentro de um ZIP). Listas usam prefixo de texto (`•`, `1.`)
//! em vez de `numbering.xml`; links relativos ficam como texto simples.

use super::images::to_png;
use super::model::{Block, Document, Inline, ListItem, Run};
use super::xml::escape;
use super::zipw::ZipWriter;
use super::{Budget, Exporter, Output};
use pulldown_cmark::Alignment;

/// Recuo, em twips (1/20 de ponto), por nível de lista ou citação.
const INDENT: usize = 540;
/// Largura útil da página A4 com margens de 2 cm, em EMU (1 px = 9525 EMU).
const MAX_IMAGE_CX: u64 = 5_940_000;
const MAX_IMAGE_CY: u64 = 7_200_000;
const EMU_PER_PX: u64 = 9_525;

pub struct DocxExporter;

impl Exporter for DocxExporter {
    fn write(&self, doc: &Document, budget: &dyn Budget) -> Result<Output, String> {
        let mut w = Writer::new(budget);
        let mut body = String::new();
        w.blocks(&doc.blocks, 0, &mut body)?;
        let bytes = w.package(doc, &body)?;
        Ok(Output { bytes, warnings: Vec::new() })
    }
}

/// Relação (hyperlink ou imagem) citada pelo `document.xml`.
struct Rel {
    id: String,
    kind: &'static str,
    target: String,
    external: bool,
}

struct Writer<'a> {
    budget: &'a dyn Budget,
    rels: Vec<Rel>,
    media: Vec<(String, Vec<u8>)>,
    drawing_id: usize,
}

fn is_web_link(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")
}

fn jc(a: Alignment) -> &'static str {
    match a {
        Alignment::Center => "<w:jc w:val=\"center\"/>",
        Alignment::Right => "<w:jc w:val=\"right\"/>",
        _ => "",
    }
}

impl<'a> Writer<'a> {
    fn new(budget: &'a dyn Budget) -> Self {
        Writer { budget, rels: Vec::new(), media: Vec::new(), drawing_id: 0 }
    }


    fn rel(&mut self, kind: &'static str, target: String, external: bool) -> String {
        // rId1 é o styles.xml.
        let id = format!("rId{}", self.rels.len() + 2);
        self.rels.push(Rel { id: id.clone(), kind, target, external });
        id
    }

    fn blocks(&mut self, blocks: &[Block], level: usize, out: &mut String) -> Result<(), String> {
        for b in blocks {
            self.block(b, level, None, false, out)?;
        }
        Ok(())
    }

    /// `prefix` é o marcador de item de lista, consumido pelo primeiro parágrafo.
    fn block(&mut self, b: &Block, level: usize, prefix: Option<&str>, quote: bool, out: &mut String) -> Result<(), String> {
        self.budget.check()?;
        let left = level * INDENT + usize::from(quote) * INDENT;
        match b {
            Block::Heading { level: l, text } => {
                let n = (*l).clamp(1, 6);
                out.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Heading{n}\"/></w:pPr>{}</w:p>", self.inlines(text, false)));
            }
            Block::Paragraph(text) => {
                let mut ppr = String::new();
                if quote {
                    ppr.push_str("<w:pBdr><w:left w:val=\"single\" w:sz=\"12\" w:space=\"8\" w:color=\"999999\"/></w:pBdr>");
                }
                if left > 0 || prefix.is_some() {
                    let hanging = if prefix.is_some() { " w:hanging=\"300\"" } else { "" };
                    let left = if prefix.is_some() { left + 300 } else { left };
                    ppr.push_str(&format!("<w:ind w:left=\"{left}\"{hanging}/>"));
                }
                let mut runs = String::new();
                if let Some(p) = prefix {
                    runs.push_str(&format!("<w:r><w:t xml:space=\"preserve\">{} </w:t></w:r>", escape(p)));
                }
                runs.push_str(&self.inlines(text, false));
                out.push_str(&format!("<w:p><w:pPr>{ppr}</w:pPr>{runs}</w:p>"));
            }
            Block::List { ordered, start, items } => {
                for (i, item) in items.iter().enumerate() {
                    self.list_item(*ordered, *start, i, item, level, quote, out)?;
                }
            }
            Block::Quote { title, blocks } => {
                if let Some(t) = title {
                    let ind = format!("<w:ind w:left=\"{}\"/>", left + INDENT);
                    out.push_str(&format!(
                        "<w:p><w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"12\" w:space=\"8\" w:color=\"999999\"/></w:pBdr>{ind}</w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>{}</w:t></w:r></w:p>",
                        escape(t)
                    ));
                }
                for inner in blocks {
                    self.block(inner, level, None, true, out)?;
                }
            }
            Block::Code { text, .. } => {
                for line in text.split('\n') {
                    let line = line.replace('\t', "    ");
                    out.push_str(&format!(
                        "<w:p><w:pPr><w:pStyle w:val=\"Code\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                        escape(&line)
                    ));
                }
            }
            Block::Table { aligns, header, rows } => self.table(aligns, header, rows, out),
            Block::Rule => out.push_str(
                "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"999999\"/></w:pBdr></w:pPr></w:p>",
            ),
            Block::Picture { alt, image } => {
                let png = to_png(image)?;
                let n = self.media.len() + 1;
                let name = format!("image{n}.png");
                self.media.push((name.clone(), png));
                let rid = self.rel(
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                    format!("media/{name}"),
                    false,
                );
                let (mut cx, mut cy) = (u64::from(image.width()) * EMU_PER_PX, u64::from(image.height()) * EMU_PER_PX);
                if cx > MAX_IMAGE_CX {
                    cy = cy * MAX_IMAGE_CX / cx;
                    cx = MAX_IMAGE_CX;
                }
                if cy > MAX_IMAGE_CY {
                    cx = cx * MAX_IMAGE_CY / cy;
                    cy = MAX_IMAGE_CY;
                }
                let (cx, cy) = (cx.max(1), cy.max(1));
                self.drawing_id += 1;
                let id = self.drawing_id;
                out.push_str(&format!(
                    "<w:p><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
<wp:docPr id=\"{id}\" name=\"Imagem {id}\" descr=\"{alt}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/>\
<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
                    alt = escape(alt)
                ));
            }
            Block::Image { alt, .. } => {
                out.push_str(&format!("<w:p><w:r><w:t>[imagem: {}]</w:t></w:r></w:p>", escape(alt)));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn list_item(&mut self, ordered: bool, start: u64, i: usize, item: &ListItem, level: usize, quote: bool, out: &mut String) -> Result<(), String> {
        let marker = match item.task {
            Some(true) => "[x]".to_string(),
            Some(false) => "[ ]".to_string(),
            None if ordered => format!("{}.", start + i as u64),
            None => "•".to_string(),
        };
        let mut pending = Some(marker);
        for inner in &item.blocks {
            match inner {
                Block::Paragraph(_) if pending.is_some() => {
                    self.block(inner, level, pending.take().as_deref(), quote, out)?;
                }
                _ => {
                    if let Some(m) = pending.take() {
                        self.block(&Block::Paragraph(vec![]), level, Some(&m), quote, out)?;
                    }
                    self.block(inner, level + 1, None, quote, out)?;
                }
            }
        }
        if let Some(m) = pending {
            self.block(&Block::Paragraph(vec![]), level, Some(&m), quote, out)?;
        }
        Ok(())
    }

    fn table(&mut self, aligns: &[Alignment], header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], out: &mut String) {
        let cols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).max(1);
        out.push_str("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"5000\" w:type=\"pct\"/></w:tblPr><w:tblGrid>");
        for _ in 0..cols {
            out.push_str(&format!("<w:gridCol w:w=\"{}\"/>", 9000 / cols));
        }
        out.push_str("</w:tblGrid>");
        let mut row = |cells: &[Vec<Inline>], head: bool, this: &mut Writer| {
            out.push_str("<w:tr>");
            for c in 0..cols {
                let empty = Vec::new();
                let cell = cells.get(c).unwrap_or(&empty);
                let a = aligns.get(c).copied().unwrap_or(Alignment::None);
                let shade = if head { "<w:tcPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"E7E6E6\"/></w:tcPr>" } else { "" };
                out.push_str(&format!(
                    "<w:tc>{shade}<w:p><w:pPr>{}</w:pPr>{}</w:p></w:tc>",
                    jc(a),
                    this.inlines(cell, head)
                ));
            }
            out.push_str("</w:tr>");
        };
        if !header.is_empty() {
            row(header, true, self);
        }
        for r in rows {
            row(r, false, self);
        }
        // Word exige um parágrafo depois da tabela.
        out.push_str("</w:tbl><w:p/>");
    }

    fn inlines(&mut self, items: &[Inline], force_bold: bool) -> String {
        let mut out = String::new();
        for item in items {
            match item {
                Inline::Break => out.push_str("<w:r><w:br/></w:r>"),
                Inline::Run(r) => out.push_str(&self.run(r, force_bold)),
            }
        }
        out
    }

    fn run(&mut self, r: &Run, force_bold: bool) -> String {
        let mut props = String::new();
        let web_link = r.link.as_deref().filter(|u| is_web_link(u));
        if web_link.is_some() {
            props.push_str("<w:rStyle w:val=\"Hyperlink\"/>");
        }
        if r.bold || force_bold {
            props.push_str("<w:b/>");
        }
        if r.italic {
            props.push_str("<w:i/>");
        }
        if r.strike {
            props.push_str("<w:strike/>");
        }
        if r.code {
            props.push_str("<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/>");
        }
        let rpr = if props.is_empty() { String::new() } else { format!("<w:rPr>{props}</w:rPr>") };
        let run = format!("<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>", escape(&r.text));
        match web_link {
            Some(url) => {
                let id = self.rel("http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink", url.to_string(), true);
                format!("<w:hyperlink r:id=\"{id}\">{run}</w:hyperlink>")
            }
            None => run,
        }
    }

    fn package(&self, doc: &Document, body: &str) -> Result<Vec<u8>, String> {
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{body}\
<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
        );
        let mut rels = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        );
        for r in &self.rels {
            let mode = if r.external { " TargetMode=\"External\"" } else { "" };
            rels.push_str(&format!("<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>", r.id, r.kind, escape(&r.target)));
        }
        rels.push_str("</Relationships>");
        let title = escape(doc.title.as_deref().unwrap_or(""));
        let core = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{title}</dc:title></cp:coreProperties>"
        );
        let mut zip = ZipWriter::new();
        zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
        zip.add("_rels/.rels", ROOT_RELS.as_bytes())?;
        zip.add("docProps/core.xml", core.as_bytes())?;
        zip.add("word/document.xml", document.as_bytes())?;
        zip.add("word/styles.xml", STYLES.as_bytes())?;
        zip.add("word/_rels/document.xml.rels", rels.as_bytes())?;
        for (name, data) in &self.media {
            zip.add(&format!("word/media/{name}"), data)?;
        }
        zip.finish()
    }
}

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
</Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
</Relationships>";

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"22\"/><w:lang w:val=\"pt-BR\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"360\" w:after=\"120\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"300\" w:after=\"100\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"30\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"2\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"26\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"3\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"24\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading5\"><w:name w:val=\"heading 5\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"4\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"22\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading6\"><w:name w:val=\"heading 6\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"5\"/></w:pPr><w:rPr><w:b/><w:i/><w:sz w:val=\"22\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F2\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/><w:sz w:val=\"18\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:rPr><w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/></w:rPr></w:style>\
<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:tblPr><w:tblBorders>\
<w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"808080\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"808080\"/>\
<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"808080\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"808080\"/>\
<w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"808080\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"808080\"/>\
</w:tblBorders></w:tblPr></w:style>\
</w:styles>";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn export(md: &str) -> Vec<u8> {
        DocxExporter.write(&parse(md, &crate::Unlimited).unwrap(), &crate::Unlimited).unwrap().bytes
    }

    fn contains(bytes: &[u8], needle: &str) -> bool {
        bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn pacote_tem_as_partes_obrigatorias() {
        let b = export("# Olá\n\ntexto\n");
        for part in ["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels"] {
            assert!(contains(&b, part), "{part}");
        }
        assert!(contains(&b, "<w:pStyle w:val=\"Heading1\"/>"));
        assert!(contains(&b, "Olá"));
    }

    #[test]
    fn estilos_listas_codigo_e_tabela() {
        let b = export("**n** *i* ~~s~~ `c`\n\n- a\n- [x] b\n\n1. um\n\n```\nfn x() {}\n```\n\n| h |\n|---|\n| v |\n");
        assert!(contains(&b, "<w:b/>") && contains(&b, "<w:i/>") && contains(&b, "<w:strike/>"));
        assert!(contains(&b, "Courier New"));
        assert!(contains(&b, "• ") && contains(&b, "[x] ") && contains(&b, "1. "));
        assert!(contains(&b, "<w:pStyle w:val=\"Code\"/>"));
        assert!(contains(&b, "<w:tbl>"));
    }

    #[test]
    fn link_web_vira_hyperlink_e_relativo_fica_texto() {
        let b = export("[a](https://x.org/?a=1&b=2) e [b](outro.md)\n");
        assert!(contains(&b, "<w:hyperlink r:id=\"rId2\">"));
        assert!(contains(&b, "TargetMode=\"External\""));
        assert!(contains(&b, "https://x.org/?a=1&amp;b=2"));
        assert!(!contains(&b, "outro.md\" TargetMode"));
    }

    #[test]
    fn xml_gerado_e_bem_formado() {
        use crate::xml::assert_balanced;
        let doc = parse("# T\n\n**a** [l](https://x.y) `c`\n\n- a\n  - b\n\n> [!NOTE]\n> q\n\n| h |\n|---|\n| v |\n\n```\nx\n```\n\n---\n", &crate::Unlimited).unwrap();
        let mut w = Writer::new(&crate::Unlimited);
        let mut body = String::new();
        w.blocks(&doc.blocks, 0, &mut body).unwrap();
        assert_balanced(&format!("<r>{body}</r>"));
        assert_balanced(STYLES);
        assert_balanced(CONTENT_TYPES);
        assert_balanced(ROOT_RELS);
    }

    #[test]
    fn texto_perigoso_e_escapado() {
        let b = export("a <b> & \"c\"\n");
        assert!(contains(&b, "&amp;"), "o & foi escapado");
        assert!(!contains(&b, " & "), "nenhum & cru");
    }

    #[test]
    fn imagem_vai_para_media_e_titulo_para_o_core() {
        let img = std::sync::Arc::new(image::DynamicImage::ImageRgb8(image::RgbImage::new(10, 5)));
        let doc = Document {
            title: Some("Meu <plano>".into()),
            blocks: vec![Block::Picture { alt: "foto".into(), image: img }],
        };
        let b = DocxExporter.write(&doc, &crate::Unlimited).unwrap().bytes;
        assert!(contains(&b, "word/media/image1.png"));
        assert!(contains(&b, "<dc:title>Meu &lt;plano&gt;</dc:title>"));
        assert!(contains(&b, "r:embed=\"rId2\""));
    }
}
