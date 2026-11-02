//! Tokenização do código de formato do Excel: separa literais, marcadores de dígito, letras de
//! data e os demais sinais. Só a primeira seção entra (até o `;` fora de aspas); negativo, zero
//! e texto ficam de fora. Cor e condição entre colchetes somem; `[$SÍMBOLO-LCID]` vira literal.

/// Campo de data ou hora que uma letra do código de formato representa.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Campo {
    Ano,
    Mes,
    Dia,
    Hora,
    Segundo,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Peca {
    /// Texto que sai como está (aspas, `\x`, espaço, símbolo de moeda, etc.).
    Literal(String),
    /// Marcador de dígito: `0`, `#` ou `?`.
    Marcador(char),
    /// Letra de data ou hora: `y`, `d`, `m`, `h` ou `s` (maiúsculas também). `[h]`, `[m]` e `[s]` também.
    Data(Campo),
    Ponto,
    Virgula,
    Percentual,
    /// Barra; vira fração quando segue um marcador (`# ?/?`).
    Barra,
    /// `E+` ou `E-`: notação científica.
    Cientifico,
    /// `@`: texto da célula.
    Texto,
}

/// Quebra a primeira seção de um código em peças. Literais vizinhos saem fundidos.
pub(crate) fn tokenizar(codigo: &str) -> Vec<Peca> {
    let mut pecas: Vec<Peca> = Vec::new();
    let mut chars = codigo.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ';' => break,
            '"' => {
                let texto: String = chars.by_ref().take_while(|x| *x != '"').collect();
                empilhar_literal(&mut pecas, &texto);
            }
            '\\' => {
                if let Some(x) = chars.next() {
                    empilhar_literal(&mut pecas, &x.to_string());
                }
            }
            '_' | '*' => {
                // `_x` (espaço do tamanho de x) e `*x` (repete x): nada aparece no texto.
                chars.next();
            }
            '[' => {
                let conteudo: String = chars.by_ref().take_while(|x| *x != ']').collect();
                if let Some(peca) = peca_de_colchete(&conteudo) {
                    pecas.push(peca);
                }
            }
            '0' | '#' | '?' => pecas.push(Peca::Marcador(c)),
            '.' => pecas.push(Peca::Ponto),
            ',' => pecas.push(Peca::Virgula),
            '%' => pecas.push(Peca::Percentual),
            '/' => pecas.push(Peca::Barra),
            '@' => pecas.push(Peca::Texto),
            'E' | 'e' if matches!(chars.peek(), Some('+' | '-')) => {
                chars.next();
                pecas.push(Peca::Cientifico);
            }
            'y' | 'Y' => pecas.push(Peca::Data(Campo::Ano)),
            'd' | 'D' => pecas.push(Peca::Data(Campo::Dia)),
            'm' | 'M' => pecas.push(Peca::Data(Campo::Mes)),
            'h' | 'H' => pecas.push(Peca::Data(Campo::Hora)),
            's' | 'S' => pecas.push(Peca::Data(Campo::Segundo)),
            outro => empilhar_literal(&mut pecas, &outro.to_string()),
        }
    }
    pecas
}

/// `[$R$-416]` vira o símbolo `R$`; `[h]`, `[mm]`, `[ss]` viram letras de tempo; o resto
/// (cor como `[Red]`, condição como `[>100]`) não aparece no texto.
fn peca_de_colchete(conteudo: &str) -> Option<Peca> {
    if let Some(moeda) = conteudo.strip_prefix('$') {
        let simbolo = moeda.split('-').next().unwrap_or("");
        return Some(Peca::Literal(simbolo.to_string()));
    }
    match conteudo.to_ascii_lowercase().as_str() {
        "h" | "hh" => Some(Peca::Data(Campo::Hora)),
        "m" | "mm" => Some(Peca::Data(Campo::Mes)),
        "s" | "ss" => Some(Peca::Data(Campo::Segundo)),
        _ => None,
    }
}

fn empilhar_literal(pecas: &mut Vec<Peca>, texto: &str) {
    if texto.is_empty() {
        return;
    }
    if let Some(Peca::Literal(anterior)) = pecas.last_mut() {
        anterior.push_str(texto);
    } else {
        pecas.push(Peca::Literal(texto.to_string()));
    }
}
