//! Pasta de trabalho do `.xlsx`: acha as abas (nome, estado e parte) e o sistema de datas
//! pelo `workbook.xml` e pelas relações dele. Os nomes das partes de textos e estilos também saem daqui.

use mdv_text::sanitize;

use crate::pacote::Pacote;
use crate::relacoes::{rels_de, resolver, Relacoes};
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

/// Uma aba como aparece no `workbook.xml`: nome já saneado, parte do pacote e se está oculta.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AbaRef {
    pub nome: String,
    pub parte: String,
    pub oculta: bool,
}

/// Abas e configurações do workbook. `textos` e `estilos` são os **nomes** das partes, quando existem.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pasta {
    pub abas: Vec<AbaRef>,
    pub data_1904: bool,
    pub textos: Option<String>,
    pub estilos: Option<String>,
}

/// Lê o workbook do pacote: abas na ordem do arquivo, sistema de datas e as partes ligadas.
/// Aba cuja relação não existe ou não é de planilha (ex.: gráfico) é descartada.
pub(crate) fn abrir(pacote: &mut Pacote<'_>, budget: &dyn Budget) -> Result<Pasta, String> {
    let principal = parte_principal(pacote, budget)?;
    let xml = pacote
        .parte(&principal)?
        .ok_or_else(|| format!("o arquivo não tem {principal} (não é uma planilha .xlsx)"))?;
    let relacoes = match pacote.parte(&rels_de(&principal))? {
        Some(rels) => Relacoes::ler(&rels, budget)?,
        None => Relacoes::vazias(),
    };
    let mut abas = Vec::new();
    let mut data_1904 = false;
    let mut leitor = Leitor::novo(&xml, budget);
    while let Some(evento) = leitor.proximo()? {
        let Evento::Abre(tag) = evento else { continue };
        match tag.nome.as_str() {
            "workbookPr" => {
                data_1904 = tag.attr("date1904").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
            }
            "sheet" => {
                let (Some(nome), Some(id)) = (tag.attr("name"), tag.attr("id")) else { continue };
                let Some(relacao) = relacoes.por_id(id) else { continue };
                if relacao.tipo != "worksheet" {
                    continue;
                }
                abas.push(AbaRef {
                    nome: sanitize(nome).into_owned(),
                    parte: resolver(&principal, &relacao.alvo),
                    oculta: matches!(tag.attr("state"), Some("hidden" | "veryHidden")),
                });
            }
            _ => {}
        }
    }
    let textos = relacoes.por_tipo("sharedStrings").map(|r| resolver(&principal, &r.alvo));
    let estilos = relacoes.por_tipo("styles").map(|r| resolver(&principal, &r.alvo));
    Ok(Pasta { abas, data_1904, textos, estilos })
}

/// Nome da parte do workbook: alvo da relação `officeDocument` da raiz, ou `xl/workbook.xml`.
fn parte_principal(pacote: &mut Pacote<'_>, budget: &dyn Budget) -> Result<String, String> {
    if let Some(raiz) = pacote.parte("_rels/.rels")? {
        let rels = Relacoes::ler(&raiz, budget)?;
        if let Some(relacao) = rels.por_tipo("officeDocument") {
            return Ok(resolver("", &relacao.alvo));
        }
    }
    Ok("xl/workbook.xml".to_string())
}

#[cfg(test)]
#[path = "pasta_tests.rs"]
mod pasta_tests;
