//! Formato de célula: aplica um código de número, percentual ou data/hora a um valor.
//! Quem decide o formato é `formato_codigo`; aqui só se junta `numero` e `data`.

use crate::data::{arredondar_segundo, serie_para_data, serie_para_hora};
use crate::numero::{fixo, geral};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Formato {
    Geral,
    Numero { decimais: u8, milhar: bool, prefixo: String, sufixo: String },
    Percentual { decimais: u8 },
    Data { data: bool, hora: bool, segundos: bool },
    Texto,
}

impl Formato {
    /// Texto que o Excel mostraria para `valor` neste formato. Não finito vira vazio.
    pub(crate) fn aplicar(&self, valor: f64, data_1904: bool) -> String {
        if !valor.is_finite() {
            return String::new();
        }
        match self {
            Formato::Geral | Formato::Texto => geral(valor),
            Formato::Numero { decimais, milhar, prefixo, sufixo } => {
                let numero = fixo(valor, *decimais, *milhar);
                match numero.strip_prefix('-') {
                    Some(resto) => format!("-{prefixo}{resto}{sufixo}"),
                    None => format!("{prefixo}{numero}{sufixo}"),
                }
            }
            Formato::Percentual { decimais } => format!("{}%", fixo(valor * 100.0, *decimais, false)),
            Formato::Data { data, hora, segundos } => {
                formatar_data(valor, data_1904, *data, *hora, *segundos)
            }
        }
    }
}

/// `AAAA-MM-DD`, `HH:MM`, `HH:MM:SS` ou `AAAA-MM-DD HH:MM[:SS]`, conforme os campos pedidos.
/// Data fora da faixa do Excel cai na regra *Geral*.
fn formatar_data(serie: f64, data_1904: bool, data: bool, hora: bool, segundos: bool) -> String {
    let base = if hora { arredondar_segundo(serie) } else { serie };
    let mut texto = String::new();
    if data {
        match serie_para_data(base, data_1904) {
            Some((ano, mes, dia)) => texto.push_str(&format!("{ano:04}-{mes:02}-{dia:02}")),
            None => return geral(serie),
        }
    }
    if hora {
        let (h, m, s) = serie_para_hora(base);
        if data {
            texto.push(' ');
        }
        texto.push_str(&format!("{h:02}:{m:02}"));
        if segundos {
            texto.push_str(&format!(":{s:02}"));
        }
    }
    texto
}
