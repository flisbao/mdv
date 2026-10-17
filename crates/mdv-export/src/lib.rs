//! Conversão de Markdown em PDF, DOCX e ODT e de tabelas (CSV, TSV, Markdown) em XLSX e ODS.
//! Biblioteca pura: recebe texto e devolve bytes. O limite de memória e as imagens vêm do
//! chamador (`Budget`, `PictureProvider`); gravar o arquivo também é com o chamador.

#[cfg(test)]
mod testutil;

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
}
