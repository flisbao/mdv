//! Conversão de Markdown em PDF, DOCX e ODT e de tabelas (CSV, TSV, Markdown) em XLSX e ODS.
//! Biblioteca pura: recebe texto e devolve bytes. O limite de memória e as imagens vêm do
//! chamador (`Budget`, `PictureProvider`); gravar o arquivo também é com o chamador.

#[cfg(test)]
mod testutil;

pub mod docx;
pub mod images;
pub mod model;
pub mod odt;
pub mod parse;
pub mod pdf;
pub mod pdf_font;
pub mod pdf_layout;
pub mod sheet;
pub mod xml;
pub mod zipw;

use std::sync::Arc;

use image::DynamicImage;

/// Limite de memória do chamador. Os laços que crescem com a entrada chamam `check`.
pub trait Budget {
    /// `Err(mensagem)` se o limite estourou.
    fn check(&self) -> Result<(), String>;
    /// Barra, antes de analisar, um texto cuja árvore do `pulldown-cmark` passaria do limite.
    fn check_parse(&self, len: usize) -> Result<(), String>;
}

/// Sem limite (testes e usos fora do `mdv`).
pub struct Unlimited;

impl Budget for Unlimited {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
    fn check_parse(&self, _len: usize) -> Result<(), String> {
        Ok(())
    }
}

/// De onde vêm as imagens de um documento. A crate só pede caminhos que passaram na política de
/// confinamento (relativos, sem `..`); o chamador decide como carregar (disco, rede, cache).
pub trait PictureProvider {
    fn load(&self, src: &str) -> Result<Arc<DynamicImage>, String>;
    /// Diagrama Mermaid desenhado. `None`: Mermaid desligado (o bloco continua código).
    fn mermaid(&self, source: &str) -> Option<Result<Arc<DynamicImage>, String>>;
}

/// Resultado de uma conversão: o arquivo pronto e avisos sobre o que não coube no formato.
pub struct Output {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Um formato de documento de texto. Para acrescentar um: implemente isto e use-o em `convert`.
pub trait Exporter {
    fn write(&self, doc: &model::Document, budget: &dyn Budget) -> Result<Output, String>;
}

/// Formatos de saída.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Docx,
    Odt,
    Xlsx,
    Ods,
}

pub const FORMATS_HELP: &str = ".pdf, .docx, .odt, .xlsx ou .ods";

impl Format {
    /// Formato da extensão `ext` (sem ponto, qualquer caixa).
    pub fn from_ext(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "pdf" => Some(Format::Pdf),
            "docx" => Some(Format::Docx),
            "odt" => Some(Format::Odt),
            "xlsx" => Some(Format::Xlsx),
            "ods" => Some(Format::Ods),
            _ => None,
        }
    }

    pub fn is_sheet(self) -> bool {
        matches!(self, Format::Xlsx | Format::Ods)
    }
}

/// O que vai ser convertido.
pub enum Input<'a> {
    Markdown(&'a str),
    /// CSV/TSV já lido. `name` vira o nome da aba (nome do arquivo sem extensão).
    Table { text: &'a str, delimiter: u8, no_header: bool, name: &'a str },
}

/// Quais tabelas de um Markdown vão para a planilha (vazio: todas).
#[derive(Default)]
pub struct Select<'a> {
    pub section: Option<&'a str>,
    pub table: Option<usize>,
}

/// O que a conversão pede ao chamador: o limite de memória e as imagens.
pub struct Context<'a> {
    pub budget: &'a dyn Budget,
    pub pictures: &'a dyn PictureProvider,
}

/// Converte `input` para `format`. Devolve os bytes do arquivo e os avisos (imagens que não
/// carregaram, caracteres que o formato não tem, cortes pelos limites do Excel). Qualquer erro de
/// combinação acontece **antes** de gerar qualquer coisa.
pub fn convert(input: &Input, select: &Select, format: Format, ctx: &Context) -> Result<Output, String> {
    let has_select = select.section.is_some() || select.table.is_some();
    match (input, format.is_sheet()) {
        (Input::Markdown(md), false) => {
            if has_select {
                return Err("--section e --table só valem para .xlsx e .ods".to_string());
            }
            let mut doc = parse::parse(md, ctx.budget)?;
            let mut warnings = images::resolve(&mut doc, ctx.pictures);
            let writer: Box<dyn Exporter> = match format {
                Format::Pdf => Box::new(pdf::PdfExporter),
                Format::Docx => Box::new(docx::DocxExporter),
                _ => Box::new(odt::OdtExporter),
            };
            let mut out = writer.write(&doc, ctx.budget)?;
            warnings.append(&mut out.warnings);
            out.warnings = warnings;
            Ok(out)
        }
        (Input::Markdown(md), true) => {
            let mut warnings = Vec::new();
            let sel = sheet::source::MdSelect { section: select.section, table: select.table };
            let wb = sheet::source::from_markdown(md, &sel, ctx.budget, &mut warnings)?;
            sheet_output(&wb, format, ctx, warnings)
        }
        (Input::Table { text, delimiter, no_header, name }, true) => {
            if has_select {
                return Err("--section e --table só valem para Markdown".to_string());
            }
            let mut warnings = Vec::new();
            let wb = sheet::source::from_csv(text, *delimiter, *no_header, name, ctx.budget, &mut warnings)?;
            sheet_output(&wb, format, ctx, warnings)
        }
        (Input::Table { .. }, false) => Err("CSV/TSV só exporta para .xlsx ou .ods".to_string()),
    }
}

fn sheet_output(wb: &sheet::model::Workbook, format: Format, ctx: &Context, mut warnings: Vec<String>) -> Result<Output, String> {
    let writer: Box<dyn sheet::SheetWriter> = match format {
        Format::Xlsx => Box::new(sheet::xlsx::XlsxWriter),
        _ => Box::new(sheet::ods::OdsWriter),
    };
    let mut out = writer.write(wb, ctx.budget)?;
    warnings.append(&mut out.warnings);
    out.warnings = warnings;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Exhausted;

    #[test]
    fn extensao_decide_o_formato_sem_olhar_a_caixa() {
        assert_eq!(Format::from_ext("PDF"), Some(Format::Pdf));
        assert_eq!(Format::from_ext("Docx"), Some(Format::Docx));
        assert_eq!(Format::from_ext("odt"), Some(Format::Odt));
        assert_eq!(Format::from_ext("XLSX"), Some(Format::Xlsx));
        assert_eq!(Format::from_ext("ods"), Some(Format::Ods));
        assert_eq!(Format::from_ext("txt"), None);
        assert_eq!(Format::from_ext(""), None);
    }

    #[test]
    fn so_xlsx_e_ods_sao_planilhas() {
        let planilhas: Vec<bool> =
            [Format::Pdf, Format::Docx, Format::Odt, Format::Xlsx, Format::Ods].iter().map(|f| f.is_sheet()).collect();
        assert_eq!(planilhas, [false, false, false, true, true]);
    }

    #[test]
    fn unlimited_nunca_recusa_e_exhausted_sempre_recusa() {
        assert!(Unlimited.check().is_ok() && Unlimited.check_parse(usize::MAX).is_ok());
        assert!(Exhausted.check().is_err() && Exhausted.check_parse(0).is_err());
    }

    use crate::testutil::{zip_entry, FakePictures};

    fn ctx<'a>(p: &'a FakePictures) -> Context<'a> {
        Context { budget: &Unlimited, pictures: p }
    }

    fn tabela(text: &str) -> Input<'_> {
        Input::Table { text, delimiter: b',', no_header: false, name: "dados" }
    }

    #[test]
    fn markdown_vira_os_tres_documentos_com_o_magic_certo() {
        let p = FakePictures::new();
        for (fmt, magic) in [(Format::Pdf, &b"%PDF"[..]), (Format::Docx, b"PK"), (Format::Odt, b"PK")] {
            let out = convert(&Input::Markdown("# Oi\n\ntexto\n"), &Select::default(), fmt, &ctx(&p)).unwrap();
            assert!(out.bytes.starts_with(magic), "{fmt:?}");
            assert!(out.warnings.is_empty(), "{fmt:?}: {:?}", out.warnings);
        }
    }

    #[test]
    fn documento_vazio_gera_arquivo_nos_tres_formatos() {
        let p = FakePictures::new();
        for fmt in [Format::Pdf, Format::Docx, Format::Odt] {
            let out = convert(&Input::Markdown(""), &Select::default(), fmt, &ctx(&p)).unwrap();
            assert!(out.bytes.len() > 100, "{fmt:?}");
        }
    }

    #[test]
    fn csv_vira_xlsx_e_ods() {
        let p = FakePictures::new();
        for fmt in [Format::Xlsx, Format::Ods] {
            let out = convert(&tabela("a,b\n1,2\n"), &Select::default(), fmt, &ctx(&p)).unwrap();
            assert!(out.bytes.starts_with(b"PK") && out.warnings.is_empty(), "{fmt:?}");
        }
        let out = convert(&tabela("a,b\n1,2\n"), &Select::default(), Format::Xlsx, &ctx(&p)).unwrap();
        assert!(zip_entry(&out.bytes, "xl/workbook.xml").contains("name=\"dados\""));
    }

    #[test]
    fn markdown_com_tabela_vira_planilha_respeitando_o_select() {
        let p = FakePictures::new();
        let md = "# A\n\n| x |\n|---|\n| 1 |\n\n# B\n\n| y |\n|---|\n| 2 |\n";
        let todas = convert(&Input::Markdown(md), &Select::default(), Format::Xlsx, &ctx(&p)).unwrap();
        assert!(zip_entry(&todas.bytes, "xl/workbook.xml").contains("name=\"B\""));
        let so_b = Select { section: Some("B"), table: None };
        let out = convert(&Input::Markdown(md), &so_b, Format::Xlsx, &ctx(&p)).unwrap();
        let wb = zip_entry(&out.bytes, "xl/workbook.xml");
        assert!(wb.contains("name=\"B\"") && !wb.contains("name=\"A\""), "{wb}");
    }

    #[test]
    fn markdown_sem_tabela_para_planilha_da_erro() {
        let p = FakePictures::new();
        let e = convert(&Input::Markdown("# so texto\n"), &Select::default(), Format::Ods, &ctx(&p)).err().unwrap();
        assert!(e.contains("não tem tabelas"), "{e}");
    }

    #[test]
    fn csv_para_documento_e_recusado() {
        let p = FakePictures::new();
        for fmt in [Format::Pdf, Format::Docx, Format::Odt] {
            let e = convert(&tabela("a\n1\n"), &Select::default(), fmt, &ctx(&p)).err().unwrap();
            assert!(e.contains(".xlsx") && e.contains(".ods"), "{e}");
        }
    }

    #[test]
    fn section_e_table_so_valem_para_planilha_de_markdown() {
        let p = FakePictures::new();
        let sel = Select { section: Some("A"), table: None };
        let e = convert(&Input::Markdown("# A\n"), &sel, Format::Pdf, &ctx(&p)).err().unwrap();
        assert!(e.contains("--section") && e.contains(".xlsx"), "{e}");
        let sel = Select { section: None, table: Some(1) };
        assert!(convert(&tabela("a\n1\n"), &sel, Format::Xlsx, &ctx(&p)).is_err());
    }

    #[test]
    fn imagens_do_documento_passam_pelo_provedor_e_avisos_voltam() {
        let p = FakePictures::failing("sumiu");
        let out = convert(&Input::Markdown("![x](a.png)\n"), &Select::default(), Format::Docx, &ctx(&p)).unwrap();
        assert!(out.warnings.iter().any(|w| w.contains("a.png") && w.contains("sumiu")), "{:?}", out.warnings);
        let p = FakePictures::new();
        convert(&Input::Markdown("![x](/etc/passwd)\n"), &Select::default(), Format::Docx, &ctx(&p)).unwrap();
        assert!(p.loads().is_empty(), "caminho absoluto nunca chega ao provedor");
    }

    #[test]
    fn budget_esgotado_aborta_todas_as_combinacoes() {
        let p = FakePictures::new();
        let c = Context { budget: &Exhausted, pictures: &p };
        for fmt in [Format::Pdf, Format::Docx, Format::Odt] {
            assert!(convert(&Input::Markdown("# a\n"), &Select::default(), fmt, &c).is_err(), "{fmt:?}");
        }
        for fmt in [Format::Xlsx, Format::Ods] {
            assert!(convert(&tabela("a\n1\n"), &Select::default(), fmt, &c).is_err(), "{fmt:?}");
            assert!(convert(&Input::Markdown("| a |\n|---|\n| 1 |\n"), &Select::default(), fmt, &c).is_err(), "{fmt:?}");
        }
    }
}
