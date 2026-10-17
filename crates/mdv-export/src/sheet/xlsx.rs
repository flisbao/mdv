//! Escritor XLSX (Office Open XML SpreadsheetML dentro de um ZIP). Textos vão como `inlineStr`,
//! sem `sharedStrings.xml`; números como `<v>`. Nunca emite `<f>`: nada vira fórmula.

use super::model::{Cell, Sheet, Workbook};
use super::SheetWriter;
use crate::xml::escape;
use crate::zipw::ZipWriter;
use crate::{Budget, Output};

pub struct XlsxWriter;

const MIN_WIDTH: usize = 10;
const MAX_WIDTH: usize = 60;
const STYLE_HEADER: u8 = 1;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
</Relationships>";

/// Estilo 0 = normal; estilo 1 = negrito (cabeçalho).
const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts>\
<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>\
<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>\
<xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/></cellXfs>\
<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>\
</styleSheet>";

/// Nome da coluna de índice `i` a partir de 0: `A`…`Z`, `AA`…, até `XFD` (16 383).
pub fn col_name(mut i: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

impl SheetWriter for XlsxWriter {
    fn write(&self, wb: &Workbook, budget: &dyn Budget) -> Result<Output, String> {
        let mut zip = ZipWriter::new();
        let mut content_types = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>",
        );
        let mut workbook = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"><sheets>"
        );
        let mut rels = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        );
        budget.check()?;
        for (i, sheet) in wb.sheets.iter().enumerate() {
            let n = i + 1;
            content_types.push_str(&format!(
                "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
            ));
            workbook.push_str(&format!("<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>", escape(&sheet.name)));
            rels.push_str(&format!(
                "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
            ));
            zip.add(&format!("xl/worksheets/sheet{n}.xml"), sheet_xml(sheet, budget)?.as_bytes())?;
        }
        let styles_rel = wb.sheets.len() + 1;
        rels.push_str(&format!(
            "<Relationship Id=\"rId{styles_rel}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>"
        ));
        content_types.push_str("</Types>");
        workbook.push_str("</sheets></workbook>");
        zip.add("[Content_Types].xml", content_types.as_bytes())?;
        zip.add("_rels/.rels", ROOT_RELS.as_bytes())?;
        zip.add("xl/workbook.xml", workbook.as_bytes())?;
        zip.add("xl/_rels/workbook.xml.rels", rels.as_bytes())?;
        zip.add("xl/styles.xml", STYLES.as_bytes())?;
        Ok(Output { bytes: zip.finish()?, warnings: Vec::new() })
    }
}

fn sheet_xml(sheet: &Sheet, budget: &dyn Budget) -> Result<String, String> {
    let ncols = sheet.rows.iter().map(Vec::len).max().unwrap_or(0);
    let dimension = if sheet.rows.is_empty() || ncols == 0 {
        "A1".to_string()
    } else {
        format!("A1:{}{}", col_name(ncols - 1), sheet.rows.len())
    };
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"{NS_MAIN}\"><dimension ref=\"{dimension}\"/>"
    );
    if sheet.header && !sheet.rows.is_empty() {
        out.push_str(
            "<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>",
        );
    }
    if ncols > 0 {
        out.push_str("<cols>");
        for c in 0..ncols {
            let chars = sheet.rows.iter().filter_map(|r| r.get(c)).map(cell_width).max().unwrap_or(0);
            let width = chars.clamp(MIN_WIDTH, MAX_WIDTH);
            out.push_str(&format!("<col min=\"{0}\" max=\"{0}\" width=\"{width}\" customWidth=\"1\"/>", c + 1));
        }
        out.push_str("</cols>");
    }
    out.push_str("<sheetData>");
    for (r, row) in sheet.rows.iter().enumerate() {
        budget.check()?;
        let n = r + 1;
        out.push_str(&format!("<row r=\"{n}\">"));
        for (c, cell) in row.iter().enumerate() {
            let at = format!("{}{n}", col_name(c));
            let style = if sheet.header && r == 0 { format!(" s=\"{STYLE_HEADER}\"") } else { String::new() };
            match cell {
                Cell::Number(v) => out.push_str(&format!("<c r=\"{at}\"{style}><v>{v}</v></c>")),
                Cell::Text(s) => out.push_str(&format!(
                    "<c r=\"{at}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                    escape(s)
                )),
            }
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData></worksheet>");
    Ok(out)
}

/// Largura aproximada em caracteres: a da linha mais comprida da célula, mais uma folga.
fn cell_width(c: &Cell) -> usize {
    match c {
        Cell::Number(v) => format!("{v}").chars().count() + 2,
        Cell::Text(s) => s.lines().map(|l| l.chars().count()).max().unwrap_or(0) + 2,
    }
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
        XlsxWriter.write(w, &Unlimited).unwrap().bytes
    }

    #[test]
    fn nomes_de_coluna() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        assert_eq!(col_name(16383), "XFD");
    }

    #[test]
    fn pacote_tem_as_partes_obrigatorias_e_xml_bem_formado() {
        let zip = export(&wb(vec![vec![t("a"), t("b")], vec![Cell::Number(1.0), t("x")]]));
        assert!(zip.starts_with(b"PK"));
        for parte in [
            "[Content_Types].xml", "_rels/.rels", "xl/workbook.xml", "xl/_rels/workbook.xml.rels",
            "xl/styles.xml", "xl/worksheets/sheet1.xml",
        ] {
            assert_balanced(&entry(&zip, parte));
        }
        assert!(entry(&zip, "xl/workbook.xml").contains("name=\"Dados\""));
    }

    #[test]
    fn texto_vai_inline_e_numero_vai_como_valor() {
        let zip = export(&wb(vec![vec![t("nome"), t("qtd")], vec![t("ana"), Cell::Number(12.5)]]));
        let s = entry(&zip, "xl/worksheets/sheet1.xml");
        assert!(s.contains("<c r=\"A2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">ana</t></is></c>"), "{s}");
        assert!(s.contains("<c r=\"B2\"><v>12.5</v></c>"), "{s}");
    }

    #[test]
    fn cabecalho_em_negrito_e_congelado() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("v")]]));
        let s = entry(&zip, "xl/worksheets/sheet1.xml");
        assert!(s.contains("<c r=\"A1\" s=\"1\" t=\"inlineStr\">"), "cabeçalho com estilo 1: {s}");
        assert!(s.contains("<pane ySplit=\"1\""), "{s}");
        let sem = export(&Workbook { sheets: vec![Sheet { name: "D".into(), header: false, rows: vec![vec![t("v")]] }] });
        let s = entry(&sem, "xl/worksheets/sheet1.xml");
        assert!(!s.contains("<pane") && !s.contains("s=\"1\""), "{s}");
    }

    #[test]
    fn formula_e_texto_perigoso_sao_literais_e_escapados() {
        let zip = export(&wb(vec![vec![t("h")], vec![t("=cmd|' /C calc'!A0")], vec![t("<b>&\"x\"</b>")]]));
        let s = entry(&zip, "xl/worksheets/sheet1.xml");
        assert!(!s.contains("<f>"), "nunca fórmula: {s}");
        assert!(s.contains("=cmd|&apos; /C calc&apos;!A0"), "{s}");
        assert!(s.contains("&lt;b&gt;&amp;&quot;x&quot;&lt;/b&gt;"), "{s}");
        assert_balanced(&s);
    }

    #[test]
    fn linhas_desiguais_e_quebra_de_linha_na_celula() {
        let zip = export(&wb(vec![vec![t("a"), t("b"), t("c")], vec![t("só um")], vec![t("l1\nl2"), t("x")]]));
        let s = entry(&zip, "xl/worksheets/sheet1.xml");
        assert!(s.contains("l1\nl2"), "quebra preservada: {s}");
        assert!(s.contains("<dimension ref=\"A1:C3\"/>"), "{s}");
        assert_balanced(&s);
    }

    #[test]
    fn planilha_sem_linhas_ainda_abre() {
        let zip = export(&Workbook { sheets: vec![Sheet { name: "Vazia".into(), header: false, rows: vec![] }] });
        let s = entry(&zip, "xl/worksheets/sheet1.xml");
        assert!(s.contains("<dimension ref=\"A1\"/>"), "{s}");
        assert_balanced(&s);
    }

    #[test]
    fn varias_abas_viram_varios_arquivos() {
        let w = Workbook {
            sheets: vec![
                Sheet { name: "Um".into(), header: false, rows: vec![vec![t("1")]] },
                Sheet { name: "Dois".into(), header: false, rows: vec![vec![t("2")]] },
            ],
        };
        let zip = export(&w);
        assert!(entry(&zip, "xl/worksheets/sheet2.xml").contains(">2<"));
        let wbxml = entry(&zip, "xl/workbook.xml");
        assert!(wbxml.contains("name=\"Um\"") && wbxml.contains("name=\"Dois\""), "{wbxml}");
        assert!(entry(&zip, "[Content_Types].xml").contains("sheet2.xml"));
    }

    #[test]
    fn largura_da_coluna_acompanha_o_conteudo_com_teto() {
        let longo = "x".repeat(300);
        let zip = export(&wb(vec![vec![t("ab"), t(&longo)]]));
        let s = entry(&zip, "xl/worksheets/sheet1.xml");
        assert!(s.contains("<col min=\"1\" max=\"1\" width=\"10\" customWidth=\"1\"/>"), "mínimo 10: {s}");
        assert!(s.contains("<col min=\"2\" max=\"2\" width=\"60\" customWidth=\"1\"/>"), "teto 60: {s}");
    }

    #[test]
    fn caractere_proibido_em_xml_vira_interrogacao() {
        let zip = export(&wb(vec![vec![t("a\u{1}b")]]));
        assert!(entry(&zip, "xl/worksheets/sheet1.xml").contains("a?b"));
    }

    #[test]
    fn budget_esgotado_aborta_com_erro() {
        let e = XlsxWriter.write(&wb(vec![vec![t("a")]]), &Exhausted).err().expect("deve falhar");
        assert!(e.contains("memória"), "{e}");
    }
}
