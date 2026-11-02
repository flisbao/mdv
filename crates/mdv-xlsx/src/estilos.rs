//! `styles.xml` → `Estilos`: índice de estilo da célula (`s`) → `Formato` de número.
//! Só `cellXfs` entra (não `cellStyleXfs`). Cada `numFmtId` distinto é classificado uma vez e
//! guardado numa tabela; cada `xf` guarda só o índice dessa tabela, então muitos `xf` com o
//! mesmo formato não multiplicam a memória.

use std::collections::HashMap;

use crate::formato::Formato;
use crate::formato_codigo::classificar;
use crate::xml_leitor::{Evento, Leitor, Tag};
use crate::Budget;

/// Quantos códigos `numFmt` são guardados; o resto fica Geral.
const MAX_NUMFMT: usize = 5000;
/// Limite de caracteres de um código de formato personalizado no próprio Excel; mais longo fica Geral.
const MAX_CODIGO_CHARS: usize = 255;
/// Quantos `xf` de `cellXfs` são aceitos; índice maior que isso fica Geral.
const MAX_XF: usize = 100_000;

static GERAL: Formato = Formato::Geral;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Estilos {
    /// Para cada `xf` (índice de estilo), posição do formato em `formatos`.
    indices: Vec<usize>,
    /// Um `Formato` por `numFmtId` distinto encontrado nos `xf`.
    formatos: Vec<Formato>,
}

impl Estilos {
    /// Sem `styles.xml`: toda célula sai no formato Geral.
    pub(crate) fn vazio() -> Estilos {
        Estilos { indices: Vec::new(), formatos: Vec::new() }
    }

    pub(crate) fn carregar(xml: &[u8], budget: &dyn Budget) -> Result<Estilos, String> {
        let mut leitor = Leitor::novo(xml, budget);
        let mut codigos: HashMap<u32, String> = HashMap::new();
        let mut posicao_do_id: HashMap<u32, usize> = HashMap::new();
        let mut formatos: Vec<Formato> = Vec::new();
        let mut indices: Vec<usize> = Vec::new();
        // O esquema do Excel grava `numFmts` antes de `cellXfs`; cada `xf` é classificado na hora em
        // que aparece e depende disso. `numFmt` só vale dentro de `numFmts` (o de `dxfs` é outra coisa).
        let (mut em_numfmts, mut em_xfs) = (false, false);
        while let Some(evento) = leitor.proximo()? {
            match evento {
                Evento::Abre(tag) => match tag.nome.as_str() {
                    "numFmts" => em_numfmts = true,
                    "numFmt" if em_numfmts => guardar_codigo(&mut codigos, &tag),
                    "cellXfs" => em_xfs = true,
                    "xf" if em_xfs && indices.len() < MAX_XF => {
                        let id = id_de(&tag, "numFmtId");
                        let posicao = *posicao_do_id.entry(id).or_insert_with(|| {
                            formatos.push(classificar(id, codigos.get(&id).map(String::as_str)));
                            formatos.len() - 1
                        });
                        indices.push(posicao);
                    }
                    _ => {}
                },
                Evento::Fecha(nome) if nome == "numFmts" => em_numfmts = false,
                Evento::Fecha(nome) if nome == "cellXfs" => em_xfs = false,
                _ => {}
            }
        }
        Ok(Estilos { indices, formatos })
    }

    /// Formato do estilo `indice_s`; índice fora da lista é Geral.
    pub(crate) fn formato(&self, indice_s: usize) -> &Formato {
        match self.indices.get(indice_s) {
            Some(&posicao) => &self.formatos[posicao],
            None => &GERAL,
        }
    }

    /// Quantos `Formato` distintos a tabela guarda (um por `numFmtId` usado).
    #[cfg(test)]
    pub(crate) fn formatos_distintos(&self) -> usize {
        self.formatos.len()
    }
}

fn guardar_codigo(codigos: &mut HashMap<u32, String>, tag: &Tag) {
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
