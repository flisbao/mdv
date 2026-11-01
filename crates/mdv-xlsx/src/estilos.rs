//! `styles.xml` → `Estilos`: índice de estilo da célula (`s`) → `Formato` de número.
//! Só `cellXfs` entra (não `cellStyleXfs`); a classificação de cada id é feita uma vez.

use std::collections::HashMap;

use crate::formato::Formato;
use crate::formato_codigo::classificar;
use crate::xml_leitor::{Evento, Leitor, Tag};
use crate::Budget;

/// Quantos códigos `numFmt` são guardados; o resto fica Geral.
const MAX_NUMFMT: usize = 5000;
/// Código mais longo que isso não é classificado (fica Geral).
const MAX_CODIGO_CHARS: usize = 5000;
/// Quantos `xf` de `cellXfs` são aceitos; índice maior que isso fica Geral.
const MAX_XF: usize = 100_000;

static GERAL: Formato = Formato::Geral;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Estilos(Vec<Formato>);

impl Estilos {
    /// Sem `styles.xml`: toda célula sai no formato Geral.
    pub(crate) fn vazio() -> Estilos {
        Estilos(Vec::new())
    }

    pub(crate) fn carregar(xml: &[u8], budget: &dyn Budget) -> Result<Estilos, String> {
        let mut leitor = Leitor::novo(xml, budget);
        let mut codigos: HashMap<u32, String> = HashMap::new();
        let mut cache: HashMap<u32, Formato> = HashMap::new();
        let mut xfs: Vec<Formato> = Vec::new();
        let mut em_xfs = false;
        while let Some(evento) = leitor.proximo()? {
            match evento {
                Evento::Abre(tag) => match tag.nome.as_str() {
                    "numFmt" => guarda_codigo(&mut codigos, &tag),
                    "cellXfs" => em_xfs = true,
                    "xf" if em_xfs && xfs.len() < MAX_XF => {
                        let id = id_de(&tag, "numFmtId");
                        let formato = cache
                            .entry(id)
                            .or_insert_with(|| classificar(id, codigos.get(&id).map(String::as_str)))
                            .clone();
                        xfs.push(formato);
                    }
                    _ => {}
                },
                Evento::Fecha(nome) if nome == "cellXfs" => em_xfs = false,
                _ => {}
            }
        }
        Ok(Estilos(xfs))
    }

    /// Formato do estilo `indice_s`; índice fora da lista é Geral.
    pub(crate) fn formato(&self, indice_s: usize) -> &Formato {
        self.0.get(indice_s).unwrap_or(&GERAL)
    }
}

fn guarda_codigo(codigos: &mut HashMap<u32, String>, tag: &Tag) {
    if codigos.len() >= MAX_NUMFMT {
        return;
    }
    if let Some(codigo) = tag.attr("formatCode") {
        if codigo.chars().count() <= MAX_CODIGO_CHARS {
            codigos.insert(id_de(tag, "numFmtId"), codigo.to_string());
        }
    }
}

/// Id numérico de um atributo; ausente ou inválido vale 0 (Geral).
fn id_de(tag: &Tag, atributo: &str) -> u32 {
    tag.attr(atributo).and_then(|v| v.parse().ok()).unwrap_or(0)
}
