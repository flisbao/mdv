//! Visualização de dados estruturados (JSON/JSONL, CSV/TSV) reaproveitando o renderizador.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::render::{error_rendered, render_code_only, Rendered, DIM};

pub const MAX_DATA_BYTES: usize = 5 * 1024 * 1024;

pub fn parse_json(text: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(v),
        Err(e) => {
            // JSONL: várias linhas, cada uma um valor.
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.len() > 1 {
                let items: Result<Vec<Value>, _> = lines.iter().map(|l| serde_json::from_str(l)).collect();
                if let Ok(items) = items {
                    return Ok(Value::Array(items));
                }
            }
            Err(format!("JSON inválido: {e} (linha {}, coluna {})", e.line(), e.column()))
        }
    }
}

pub fn select<'a>(v: &'a Value, path: &str) -> Result<&'a Value, String> {
    let mut cur = v;
    let mut rest = path.trim();
    if rest.is_empty() || rest == "." {
        return Ok(cur);
    }
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            let key = &r[..end];
            if key.is_empty() {
                return Err("chave vazia no caminho".into());
            }
            cur = cur.get(key).ok_or_else(|| format!("a chave '{key}' não existe"))?;
            rest = &r[end..];
        } else if let Some(r) = rest.strip_prefix('[') {
            let close = r.find(']').ok_or("falta ']' no caminho")?;
            let idx: usize = r[..close].parse().map_err(|_| format!("índice inválido: {}", &r[..close]))?;
            cur = cur.get(idx).ok_or_else(|| format!("o índice {idx} não existe"))?;
            rest = &r[close + 1..];
        } else {
            return Err(format!("caminho inválido perto de '{rest}' (use .chave e [n])"));
        }
    }
    Ok(cur)
}

pub fn describe(v: &Value) -> String {
    match v {
        Value::Object(m) => format!("objeto · {} chaves", m.len()),
        Value::Array(a) => format!("lista · {} itens", a.len()),
        Value::String(_) => "texto".into(),
        Value::Number(_) => "número".into(),
        Value::Bool(_) => "booleano".into(),
        Value::Null => "nulo".into(),
    }
}

pub fn render_json(text: &str, path: Option<&str>, width: usize) -> Rendered {
    if text.len() > MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo JSON (limite de 5 MiB)");
    }
    let root = match parse_json(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let sel = match select(&root, path.unwrap_or("")) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let pretty = serde_json::to_string_pretty(sel).unwrap_or_default();
    let mut r = render_code_only(&pretty, "json", width);
    let head = Line::from(Span::styled(
        describe(sel),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    ));
    r.lines.insert(0, head);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(r: &crate::render::Rendered) -> String {
        r.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn select_navega_chaves_e_indices() {
        let v = json!({"a": {"b": [10, {"c": "x"}]}});
        assert_eq!(select(&v, ".a.b[1].c").unwrap(), &json!("x"));
        assert_eq!(select(&v, ".a.b[0]").unwrap(), &json!(10));
        assert_eq!(select(&v, "").unwrap(), &v);
        assert_eq!(select(&v, ".").unwrap(), &v);
    }

    #[test]
    fn select_erros_claros() {
        let v = json!({"a": [1]});
        assert!(select(&v, ".x").unwrap_err().contains("'x'"));
        assert!(select(&v, ".a[5]").unwrap_err().contains('5'));
        assert!(select(&v, ".a[").is_err());
        assert!(select(&v, "a").is_err());
    }

    #[test]
    fn jsonl_vira_lista() {
        let v = parse_json("{\"a\":1}\n{\"a\":2}\n").unwrap();
        assert_eq!(v, json!([{"a": 1}, {"a": 2}]));
    }

    #[test]
    fn json_invalido_informa_linha_e_coluna() {
        let e = parse_json("{\n  \"a\": ,\n}").unwrap_err();
        assert!(e.contains("linha 2"), "{e}");
    }

    #[test]
    fn describe_resume_tipo_e_tamanho() {
        assert_eq!(describe(&json!({"a": 1, "b": 2})), "objeto · 2 chaves");
        assert_eq!(describe(&json!([1, 2, 3])), "lista · 3 itens");
        assert_eq!(describe(&json!("x")), "texto");
        assert_eq!(describe(&json!(null)), "nulo");
    }

    #[test]
    fn render_json_mostra_resumo_e_conteudo_indentado() {
        let t = text(&render_json("{\"nome\":\"mdv\",\"v\":[1,2]}", None, 60));
        assert!(t.contains("objeto · 2 chaves"), "{t}");
        assert!(t.contains("\"nome\": \"mdv\""), "{t}");
    }

    #[test]
    fn render_json_com_caminho_mostra_so_o_trecho() {
        let t = text(&render_json("{\"a\":{\"b\":1},\"c\":2}", Some(".a"), 60));
        assert!(t.contains("\"b\": 1") && !t.contains("\"c\""), "{t}");
    }

    #[test]
    fn render_json_erro_vira_linha_de_erro() {
        let r = render_json("{", None, 60);
        assert_eq!(r.lines.len(), 1);
        assert!(text(&r).contains("JSON"));
    }

    #[test]
    fn controle_em_string_nao_vaza_para_o_terminal() {
        let r = render_json("{\"x\":\"\\u001b]0;pwn\\u0007\"}", None, 60);
        let ansi = crate::render::to_ansi(&r.lines, false);
        assert!(!ansi.contains('\x1b'), "{ansi:?}");
    }

    #[test]
    fn entrada_grande_e_recusada() {
        let big = format!("[{}]", "1,".repeat(MAX_DATA_BYTES));
        assert!(text(&render_json(&big, None, 60)).contains("grande"));
    }
}
