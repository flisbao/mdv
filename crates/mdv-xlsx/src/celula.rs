//! Valor cru de uma célula + tipo (`t`) + formato do estilo → `Celula` (texto e se é numérica).
//! A regra de tipo mora aqui; quem lê a aba é que aplica `sanitize` e o corte (`texto_seguro`).

use crate::formato::Formato;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Celula {
    pub texto: String,
    pub numerica: bool,
}

/// Texto de uma célula conforme o tipo `t` (`None` = número). `valor` vazio é sempre célula vazia
/// (fórmula sem valor em cache). `s` com índice fora da lista também fica vazio.
pub(crate) fn converter(tipo: Option<&str>, valor: &str, formato: &Formato, textos: &[String], data_1904: bool) -> Celula {
    if valor.is_empty() {
        return Celula::default();
    }
    match tipo {
        Some("s") => {
            let indice: Option<usize> = valor.trim().parse().ok();
            texto(indice.and_then(|i| textos.get(i)).cloned().unwrap_or_default())
        }
        Some("b") => texto(match valor.trim() {
            "1" => "VERDADEIRO".to_string(),
            "0" => "FALSO".to_string(),
            _ => valor.to_string(),
        }),
        // `str` e `inlineStr` já são texto; `e` é o nome do erro (`#DIV/0!`); `d` é ISO e fica como está.
        Some("str" | "inlineStr" | "e" | "d") => texto(valor.to_string()),
        None | Some("n") => match valor.trim().parse::<f64>() {
            Ok(numero) if numero.is_finite() => Celula { texto: formato.aplicar(numero, data_1904), numerica: true },
            _ => texto(valor.to_string()),
        },
        Some(_) => texto(valor.to_string()),
    }
}

/// Texto que entra na saída: sem caracteres de controle e cortado em `max` caracteres (+ `…`).
pub(crate) fn texto_seguro(bruto: &str, max: usize) -> String {
    cortar(&mdv_text::sanitize(bruto), max)
}

/// Acrescenta `trecho` a `destino` sem passar de `max_chars + 1` caracteres; o caractere extra
/// marca o corte. O conteúdo é o que `sanitize` daria ao texto inteiro: caractere a caractere, e um
/// `\r` no fim fica pendente (vira `\n` se vier `\n` a seguir, senão U+FFFD). Quem usa
/// `destino` passa por `texto_seguro`, que sanitiza de novo e corta. Devolve `true` se passou de `max_chars`.
pub(crate) fn acumular_limitado(destino: &mut String, trecho: &str, max_chars: usize) -> bool {
    let limite = max_chars + 1;
    let mut contagem = destino.chars().count();
    for c in trecho.chars() {
        if contagem >= limite {
            break;
        }
        if destino.ends_with('\r') {
            destino.pop();
            if c == '\n' {
                destino.push('\n');
                continue;
            }
            destino.push('\u{FFFD}');
        }
        if c == '\r' {
            destino.push('\r');
        } else {
            let mut buf = [0u8; 4];
            destino.push_str(&mdv_text::sanitize(c.encode_utf8(&mut buf)));
        }
        contagem += 1;
    }
    contagem > max_chars
}

fn cortar(texto: &str, max: usize) -> String {
    match texto.char_indices().nth(max) {
        Some((fim, _)) => format!("{}…", &texto[..fim]),
        None => texto.to_string(),
    }
}

fn texto(texto: String) -> Celula {
    Celula { texto, numerica: false }
}
