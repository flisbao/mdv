//! Escritor ODS (OpenDocument Spreadsheet dentro de um ZIP). O `mimetype` vai primeiro e sem
//! compressão (o `ZipWriter` nunca comprime). Nunca emite `table:formula`: nada vira fórmula.

use super::model::{Cell, Sheet, Workbook};
use super::SheetWriter;
use crate::xml::escape;
use crate::zipw::ZipWriter;
use crate::{Budget, Output};

pub struct OdsWriter;

const MIMETYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";

const NAMESPACES: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.2\"";

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
<manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/>\
</manifest:manifest>";

const META: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" office:version=\"1.2\">\
<office:meta><meta:generator>mdv</meta:generator></office:meta></office:document-meta>";

const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" office:version=\"1.2\"/>";

impl SheetWriter for OdsWriter {
    fn write(&self, wb: &Workbook, budget: &dyn Budget) -> Result<Output, String> {
        budget.check()?;
        let mut content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content {NAMESPACES}>\
<office:automatic-styles><style:style style:name=\"hd\" style:family=\"table-cell\">\
<style:text-properties fo:font-weight=\"bold\"/></style:style></office:automatic-styles>\
<office:body><office:spreadsheet>"
        );
        for sheet in &wb.sheets {
            sheet_xml(sheet, budget, &mut content)?;
        }
        content.push_str("</office:spreadsheet></office:body></office:document-content>");
        let mut zip = ZipWriter::new();
        zip.add("mimetype", MIMETYPE.as_bytes())?;
        zip.add("META-INF/manifest.xml", MANIFEST.as_bytes())?;
        zip.add("content.xml", content.as_bytes())?;
        zip.add("styles.xml", STYLES.as_bytes())?;
        zip.add("meta.xml", META.as_bytes())?;
        Ok(Output { bytes: zip.finish()?, warnings: Vec::new() })
    }
}

fn sheet_xml(sheet: &Sheet, budget: &dyn Budget, out: &mut String) -> Result<(), String> {
    let ncols = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
    out.push_str(&format!("<table:table table:name=\"{}\">", escape(&sheet.name)));
    if ncols > 0 {
        out.push_str(&format!("<table:table-column table:number-columns-repeated=\"{ncols}\"/>"));
    }
    for (r, row) in sheet.rows.iter().enumerate() {
        budget.check()?;
        out.push_str("<table:table-row>");
        let style = if sheet.header && r == 0 { " table:style-name=\"hd\"" } else { "" };
        for cell in row {
            match cell {
                Cell::Number(v) => out.push_str(&format!(
                    "<table:table-cell{style} office:value-type=\"float\" office:value=\"{v}\"><text:p>{v}</text:p></table:table-cell>"
                )),
                Cell::Text(s) => out.push_str(&format!(
                    "<table:table-cell{style} office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>",
                    text_xml(s)
                )),
            }
        }
        for _ in row.len()..ncols {
            out.push_str("<table:table-cell/>");
        }
        out.push_str("</table:table-row>");
    }
    out.push_str("</table:table>");
    Ok(())
}

/// Texto de célula em ODF: espaços repetidos viram `<text:s>`, tab vira `<text:tab/>` e quebra de
/// linha vira `<text:line-break/>` (o ODF colapsa espaços e ignora `\n` solto). Um espaço único no
/// meio do texto fica literal; no começo, ou em sequência de 2 ou mais, vira `<text:s>`.
fn text_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut spaces = 0usize;
    let mut first = true;
    let flush = |out: &mut String, spaces: &mut usize, first: bool| {
        match *spaces {
            0 => {}
            1 if !first => out.push(' '),
            n => out.push_str(&format!("<text:s text:c=\"{n}\"/>")),
        }
        *spaces = 0;
    };
    for c in s.chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        flush(&mut out, &mut spaces, first);
        first = false;
        match c {
            '\t' => out.push_str("<text:tab/>"),
            '\n' => out.push_str("<text:line-break/>"),
            c => out.push_str(&escape(c.encode_utf8(&mut [0; 4]))),
        }
    }
    if spaces > 0 {
        out.push_str(&format!("<text:s text:c=\"{spaces}\"/>"));
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::model::{Cell, Sheet};
    use crate::testutil::{zip_entry as entry, Exhausted};
    use crate::xml::assert_balanced;
    use crate::Unlimited;

    fn t(s: &str) -> Cell {
        Cell::Text(s.to_string())
    }

    fn wb(rows: Vec<Vec<Cell>>) -> Workbook {
        Workbook { sheets: vec![Sheet { name: "Dados".into(), header: true, rows }] }
    }

    fn export(w: &Workbook) -> Vec<u8> {
        OdsWriter.write(w, &Unlimited).unwrap().bytes
    }

    #[test]
    fn mimetype_e_a_primeira_entrada_sem_compressao() {
        let zip = export(&wb(vec![vec![t("a")]]));
        assert_eq!(&zip[0..4], &[0x50, 0x4b, 0x03, 0x04]);
        assert_eq!(&zip[8..10], &[0, 0], "método stored");
        assert_eq!(&zip[30..38], b"mimetype");
        assert_eq!(entry(&zip, "mimetype"), "application/vnd.oasis.opendocument.spreadsheet");
    }

    #[test]
    fn pacote_tem_as_partes_e_xml_bem_formado() {
        let zip = export(&wb(vec![vec![t("a")], vec![Cell::Number(2.0)]]));
        for parte in ["META-INF/manifest.xml", "content.xml", "styles.xml", "meta.xml"] {
            assert_balanced(&entry(&zip, parte));
        }
        let manifest = entry(&zip, "META-INF/manifest.xml");
        assert!(manifest.contains("application/vnd.oasis.opendocument.spreadsheet"), "{manifest}");
        assert!(entry(&zip, "content.xml").contains("table:name=\"Dados\""));
    }

    #[test]
    fn texto_e_numero_tem_tipos_distintos() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("ana"), Cell::Number(12.5)]]));
        let c = entry(&zip, "content.xml");
        assert!(c.contains("<table:table-cell office:value-type=\"string\"><text:p>ana</text:p></table:table-cell>"), "{c}");
        assert!(c.contains("<table:table-cell office:value-type=\"float\" office:value=\"12.5\"><text:p>12.5</text:p></table:table-cell>"), "{c}");
    }

    #[test]
    fn cabecalho_usa_estilo_negrito_so_na_primeira_linha() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("v")]]));
        let c = entry(&zip, "content.xml");
        assert!(c.contains("table:style-name=\"hd\" office:value-type=\"string\"><text:p>h</text:p>"), "{c}");
        assert!(!c.contains("table:style-name=\"hd\" office:value-type=\"string\"><text:p>v</text:p>"), "{c}");
        assert!(c.contains("fo:font-weight=\"bold\""), "{c}");
    }

    #[test]
    fn formula_e_texto_perigoso_sao_literais_e_escapados() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("=1+1")], vec![t("<b>&</b>")]]));
        let c = entry(&zip, "content.xml");
        assert!(!c.contains("table:formula"), "nunca fórmula: {c}");
        assert!(c.contains("<text:p>=1+1</text:p>"), "{c}");
        assert!(c.contains("&lt;b&gt;&amp;&lt;/b&gt;"), "{c}");
        assert_balanced(&c);
    }

    #[test]
    fn quebra_de_linha_vira_text_line_break() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("l1\nl2")]]));
        let c = entry(&zip, "content.xml");
        assert!(c.contains("l1<text:line-break/>l2"), "{c}");
        assert_balanced(&c);
    }

    #[test]
    fn espacos_e_tabs_sao_preservados() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("  a\tb  ")]]));
        let c = entry(&zip, "content.xml");
        assert!(c.contains("<text:s text:c=\"2\"/>a<text:tab/>b<text:s text:c=\"2\"/>"), "{c}");
        assert_balanced(&c);
    }

    #[test]
    fn linhas_desiguais_sao_completadas_com_celulas_vazias() {
        let zip = export(&wb(vec![vec![t("a"), t("b"), t("c")], vec![t("x")]]));
        let c = entry(&zip, "content.xml");
        assert!(c.contains("<text:p>x</text:p></table:table-cell><table:table-cell/><table:table-cell/></table:table-row>"), "{c}");
        assert!(c.contains("<table:table-column table:number-columns-repeated=\"3\"/>"), "{c}");
    }

    #[test]
    fn varias_abas_e_aba_vazia() {
        let w = Workbook {
            sheets: vec![
                Sheet { name: "Um".into(), header: false, rows: vec![vec![t("1")]] },
                Sheet { name: "Vazia".into(), header: false, rows: vec![] },
            ],
        };
        let c = entry(&export(&w), "content.xml");
        assert!(c.contains("table:name=\"Um\"") && c.contains("table:name=\"Vazia\""), "{c}");
        assert_balanced(&c);
    }

    #[test]
    fn budget_esgotado_aborta_com_erro() {
        let e = OdsWriter.write(&wb(vec![vec![t("a")]]), &Exhausted).err().expect("deve falhar");
        assert!(e.contains("memória"), "{e}");
    }
}
