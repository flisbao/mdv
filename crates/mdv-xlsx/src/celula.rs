//! Valor cru de uma célula + tipo (`t`) + formato do estilo → `Celula` (texto e se é numérica).
//! A regra de tipo mora aqui; `planilha::ler` é o único ponto que aplica `sanitize` e o corte
//! (`texto_limitado::texto_seguro`) ao texto que sai daqui.

use crate::modelo::Celula;
use crate::planilha::ContextoPasta;

/// Texto de uma célula conforme o tipo `t` (`None` = número). `valor` vazio é sempre célula vazia
/// (fórmula sem valor em cache). `s` com índice fora da lista também fica vazio. Os textos
/// compartilhados, o formato do `estilo` e o sistema de datas vêm do contexto da pasta.
pub(crate) fn interpretar(tipo: Option<&str>, valor: &str, estilo: usize, ctx: &ContextoPasta) -> Celula {
    if valor.is_empty() {
        return Celula::default();
    }
    match tipo {
        Some("s") => {
            let indice: Option<usize> = valor.trim().parse().ok();
            texto(indice.and_then(|i| ctx.textos.get(i)).cloned().unwrap_or_default())
        }
        Some("b") => texto(match valor.trim() {
            "1" => "VERDADEIRO".to_string(),
            "0" => "FALSO".to_string(),
            _ => valor.to_string(),
        }),
        // `str` e `inlineStr` já são texto; `e` é o nome do erro (`#DIV/0!`); `d` é ISO e fica como está.
        Some("str" | "inlineStr" | "e" | "d") => texto(valor.to_string()),
        None | Some("n") => match valor.trim().parse::<f64>() {
            Ok(numero) if numero.is_finite() => Celula { texto: ctx.estilos.formato(estilo).aplicar(numero, ctx.data_1904), numerica: true },
            _ => texto(valor.to_string()),
        },
        Some(_) => texto(valor.to_string()),
    }
}

fn texto(texto: String) -> Celula {
    Celula { texto, numerica: false }
}
