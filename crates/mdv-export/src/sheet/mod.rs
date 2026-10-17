//! Exportação de tabelas para planilha (XLSX e ODS): o modelo, as fontes (CSV/TSV e tabelas do
//! Markdown) e os dois escritores.
pub mod model;
pub mod ods;
pub mod source;
pub mod xlsx;

use super::{Budget, Output};
use model::Workbook;

/// Um formato de planilha. Para acrescentar um: implemente isto e use-o em `convert`.
pub trait SheetWriter {
    fn write(&self, wb: &Workbook, budget: &dyn Budget) -> Result<Output, String>;
}
