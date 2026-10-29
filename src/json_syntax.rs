//! Leitura de JSON/JSONL com erro de sintaxe localizado: posição em linha e coluna do arquivo e
//! mensagem em português (o `serde_json` só entrega texto em inglês).

use serde_json::Value;

use crate::mem;
use crate::syntax_error::SyntaxError;

/// Lê `text` como um documento JSON; se falhar e o texto for uma lista de valores (JSONL), lê como
/// `Value::Array`. O erro aponta o trecho errado do arquivo (o BOM inicial não desloca a numeração).
pub fn parse(text: &str) -> Result<Value, SyntaxError> {
    let text = text.trim_start_matches('\u{feff}');
    let whole = match serde_json::from_str::<Value>(text) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    match parse_jsonl(text) {
        Some(Ok(items)) => Ok(Value::Array(items)),
        Some(Err(e)) => Err(e),
        None => Err(located(text, &whole)),
    }
}

/// JSONL: vale só com ao menos duas linhas não vazias e a primeira sendo um valor completo.
/// `None` quando o texto não tem esse formato (vale o erro do documento inteiro).
fn parse_jsonl(text: &str) -> Option<Result<Vec<Value>, SyntaxError>> {
    let mut rows = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty());
    let (_, first) = rows.next()?;
    serde_json::from_str::<Value>(first).ok()?;
    rows.clone().next()?;
    let mut items = Vec::new();
    for (i, l) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        if let Err(e) = mem::check() {
            return Some(Err(SyntaxError { line: i + 1, col: 1, message: e.to_string() }));
        }
        match serde_json::from_str::<Value>(l) {
            Ok(v) => items.push(v),
            Err(e) => {
                let col = char_col(l, e.column());
                return Some(Err(SyntaxError { line: i + 1, col, message: message_pt(&e, true) }));
            }
        }
    }
    Some(Ok(items))
}

/// Erro do documento inteiro com a posição ajustada: coluna em caracteres (o `serde_json` conta
/// bytes) e, quando o parser para logo após uma quebra (coluna 0), o fim da linha anterior.
fn located(text: &str, e: &serde_json::Error) -> SyntaxError {
    if text.trim().is_empty() {
        return SyntaxError { line: 1, col: 1, message: "documento vazio".into() };
    }
    let mut line = e.line().max(1);
    let mut col = e.column();
    if col == 0 && line > 1 {
        line -= 1;
        let prev = text.split('\n').nth(line - 1).unwrap_or("");
        col = prev.strip_suffix('\r').unwrap_or(prev).chars().count() + 1;
    } else {
        let l = text.split('\n').nth(line - 1).unwrap_or("");
        col = char_col(l, col);
    }
    SyntaxError { line, col, message: message_pt(e, false) }
}

/// Coluna (1-based, em bytes, como o `serde_json`) convertida para caracteres dentro de `line`.
fn char_col(line: &str, col: usize) -> usize {
    let mut at = col.saturating_sub(1).min(line.len());
    while !line.is_char_boundary(at) {
        at -= 1;
    }
    line[..at].chars().count() + 1
}

/// Mensagem do `serde_json` em português, sem a posição. `jsonl` troca "arquivo" por "linha".
fn message_pt(e: &serde_json::Error, jsonl: bool) -> String {
    if e.classify() == serde_json::error::Category::Eof {
        let onde = if jsonl { "linha incompleta" } else { "fim inesperado do arquivo" };
        return format!("{onde} (falta fechar {{ [ ou \")");
    }
    let full = e.to_string();
    let en = full.rfind(" at line ").map_or(full.as_str(), |i| &full[..i]);
    let pt = match en {
        "expected value" => "valor esperado (texto, número, true, false, null, { ou [)",
        "expected `,` or `}`" => "esperado `,` ou `}`",
        "expected `,` or `]`" => "esperado `,` ou `]`",
        "expected `:`" => "esperado `:` depois da chave",
        "trailing comma" => "vírgula sobrando antes de fechar (remova a última vírgula)",
        "key must be a string" => "a chave precisa estar entre aspas",
        "trailing characters" => "texto sobrando depois do fim do valor",
        "invalid escape" => "escape inválido dentro de um texto",
        "expected ident" => "valor inválido (esperado true, false ou null)",
        "invalid number" => "número inválido",
        "number out of range" => "número grande demais",
        "invalid unicode code point" => "código Unicode inválido depois de \\u",
        "unexpected end of hex escape" => "escape \\u incompleto (são 4 dígitos hexadecimais)",
        s if s.contains("surrogate") => "escape \\u inválido (par substituto incompleto)",
        "recursion limit exceeded" => "estrutura aninhada demais",
        s if s.starts_with("control character") => "quebra de linha ou caractere de controle dentro de um texto (use \\n)",
        s => return format!("sintaxe inválida: {}", s.split_whitespace().collect::<Vec<_>>().join(" ")),
    };
    pt.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn erro(text: &str) -> SyntaxError {
        parse(text).expect_err("deveria falhar")
    }

    #[test]
    fn virgula_sobrando_aponta_a_linha() {
        let e = erro("{\n  \"a\": 1,\n}\n");
        assert_eq!(e.line, 3);
        assert!(e.message.contains("vírgula"), "{}", e.message);
    }

    #[test]
    fn chave_sem_aspas_aponta_a_linha_e_coluna() {
        let e = erro("{\n  a: 1\n}");
        assert_eq!((e.line, e.col), (2, 3));
    }

    #[test]
    fn fim_inesperado_do_arquivo() {
        let e = erro("{\"a\": [1, 2");
        assert!(e.message.contains("fim inesperado"), "{}", e.message);
    }

    #[test]
    fn jsonl_com_linha_ruim_aponta_a_linha_ruim() {
        let e = erro("{\"a\":1}\n{\"a\":2}\n{\"a\":\n{\"a\":4}\n");
        assert_eq!(e.line, 3);
    }

    #[test]
    fn jsonl_valido_continua_lista() {
        let v = parse("{\"a\":1}\n{\"a\":2}\n").unwrap();
        assert_eq!(v, serde_json::json!([{"a": 1}, {"a": 2}]));
    }

    #[test]
    fn bom_e_crlf_nao_deslocam_a_linha() {
        let e = erro("\u{feff}{\r\n  \"a\": 1,\r\n}\r\n");
        assert_eq!(e.line, 3);
    }

    #[test]
    fn coluna_conta_caracteres_e_nao_bytes() {
        let e = erro("{\"é\": a}");
        assert_eq!((e.line, e.col), (1, 7));
    }

    #[test]
    fn quebra_dentro_de_texto_aponta_o_fim_da_linha_e_nao_a_coluna_zero() {
        let e = erro("{\"a\": \"x\ny\"}");
        assert_eq!((e.line, e.col), (1, 9));
        assert!(e.message.contains("controle"), "{}", e.message);
    }

    #[test]
    fn jsonl_incompleto_diz_linha_e_nao_arquivo() {
        let e = erro("{\"a\":1}\n{\"a\":");
        assert_eq!(e.line, 2);
        assert!(e.message.starts_with("linha incompleta"), "{}", e.message);
    }

    #[test]
    fn texto_vazio_e_erro_na_linha_um() {
        let e = erro("");
        assert_eq!((e.line, e.col), (1, 1));
        assert_eq!(e.message, "documento vazio");
        assert_eq!(erro(" \n\t\n").message, "documento vazio");
    }

    #[test]
    fn mensagens_conhecidas_saem_em_portugues() {
        for (texto, trecho) in [
            ("[tru]", "esperado true"),
            ("[1.]", "número inválido"),
            ("[\"\\ud800\"]", "escape \\u"),
            ("[\"\\u12\"]", "escape inválido"),
            ("[\"\\x\"]", "escape inválido"),
        ] {
            let e = erro(texto);
            assert!(e.message.contains(trecho), "{texto}: {}", e.message);
            assert!(!e.message.starts_with("sintaxe inválida"), "{texto}: {}", e.message);
        }
    }

    #[test]
    fn jsonl_com_multibyte_crlf_e_sem_quebra_final() {
        let e = erro("{\"é\":1}\r\n{\"é\": x}");
        assert_eq!((e.line, e.col), (2, 7));
        let v = parse("{\"é\":1}\r\n{\"é\":2}").unwrap();
        assert_eq!(v, serde_json::json!([{"é": 1}, {"é": 2}]));
    }
}
