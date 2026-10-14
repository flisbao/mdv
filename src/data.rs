//! Visualização de dados estruturados (JSON/JSONL, CSV/TSV) reaproveitando o renderizador.

use pulldown_cmark::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::render::{error_rendered, render_code_only, render_table_only, Rendered, DIM};

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

/// Teto do JSON já indentado: a indentação multiplica uma entrada pequena e muito aninhada.
const MAX_PRETTY_BYTES: usize = 8 * 1024 * 1024;

/// Escreve só até `MAX_PRETTY_BYTES`; passar disso é erro (o valor nunca é montado inteiro).
struct BoundedBuf(Vec<u8>);

impl std::io::Write for BoundedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + buf.len() > MAX_PRETTY_BYTES {
            return Err(std::io::Error::new(std::io::ErrorKind::OutOfMemory, "saída grande demais"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn pretty_bounded(v: &Value) -> Option<String> {
    let mut out = BoundedBuf(Vec::new());
    serde_json::to_writer_pretty(&mut out, v).ok()?;
    String::from_utf8(out.0).ok()
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
    let Some(pretty) = pretty_bounded(sel) else {
        return error_rendered("JSON grande demais depois de indentado (limite de 8 MiB de saída)");
    };
    let mut r = render_code_only(&pretty, "json", width);
    let head = Line::from(Span::styled(
        describe(sel),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    ));
    r.lines.insert(0, head);
    r
}

pub struct TableOpts {
    pub delimiter: u8,
    pub head: usize,
    pub cols: Vec<String>,
    pub no_header: bool,
}

fn is_numeric_column(rows: &[Vec<String>], c: usize, skip_header: bool) -> bool {
    let cells: Vec<&str> = rows
        .iter()
        .skip(usize::from(skip_header))
        .filter_map(|r| r.get(c).map(String::as_str))
        .filter(|s| !s.trim().is_empty())
        .collect();
    !cells.is_empty() && cells.iter().all(|s| s.trim().parse::<f64>().is_ok())
}

pub fn render_table_data(text: &str, o: &TableOpts, width: usize) -> Rendered {
    if text.len() > MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo tabela (limite de 5 MiB)");
    }
    let text = text.trim_start_matches('\u{feff}');
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(o.delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    let keep = o.head + usize::from(!o.no_header);
    let (mut rows, mut total): (Vec<Vec<String>>, usize) = (Vec::new(), 0);
    for rec in rdr.records() {
        let rec = match rec {
            Ok(r) => r,
            Err(e) => return error_rendered(&format!("CSV inválido: {e}")),
        };
        total += 1;
        if rows.len() < keep {
            rows.push(rec.iter().map(str::to_string).collect());
        }
    }
    if rows.is_empty() {
        return error_rendered("arquivo vazio");
    }
    let has_header = !o.no_header;
    let data_total = total - usize::from(has_header);

    if !o.cols.is_empty() {
        if !has_header {
            return error_rendered("--cols precisa de cabeçalho (remova --no-header)");
        }
        let header = rows[0].clone();
        let mut idx = Vec::new();
        for name in &o.cols {
            if let Some(i) = header.iter().position(|h| h == name) {
                idx.push(i);
            } else {
                return error_rendered(&format!(
                    "a coluna '{name}' não existe (disponíveis: {})",
                    header.join(", ")
                ));
            }
        }
        rows = rows
            .into_iter()
            .map(|r| idx.iter().map(|&i| r.get(i).cloned().unwrap_or_default()).collect())
            .collect();
    }

    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let aligns: Vec<Alignment> = (0..ncols)
        .map(|c| if is_numeric_column(&rows, c, has_header) { Alignment::Right } else { Alignment::Left })
        .collect();
    let shown = rows.len() - usize::from(has_header);
    let mut r = render_table_only(rows, has_header, aligns, width);
    r.lines.push(Line::from(Span::styled(
        format!("{shown} de {data_total} linhas · {ncols} colunas"),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));
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

    fn opts() -> TableOpts {
        TableOpts { delimiter: b',', head: 1000, cols: vec![], no_header: false }
    }

    #[test]
    fn csv_vira_tabela_com_cabecalho_e_rodape() {
        let t = text(&render_table_data("id,nome\n1,Ana\n2,Bia\n", &opts(), 60));
        assert!(t.contains("id") && t.contains("Ana") && t.contains("Bia"), "{t}");
        assert!(t.contains("2 de 2 linhas") && t.contains("2 colunas"), "{t}");
    }

    #[test]
    fn tsv_usa_tab() {
        let o = TableOpts { delimiter: b'\t', ..opts() };
        let t = text(&render_table_data("a\tb\n1\t2\n", &o, 60));
        assert!(t.contains("│ 1") || t.contains("1 │"), "{t}");
    }

    #[test]
    fn head_limita_linhas_e_informa_total() {
        let o = TableOpts { head: 2, ..opts() };
        let t = text(&render_table_data("n\n1\n2\n3\n4\n", &o, 40));
        assert!(t.contains("2 de 4 linhas"), "{t}");
        assert!(!t.contains("│ 4"), "{t}");
    }

    #[test]
    fn cols_seleciona_por_nome_e_valida() {
        let o = TableOpts { cols: vec!["nome".into()], ..opts() };
        let t = text(&render_table_data("id,nome\n1,Ana\n", &o, 40));
        assert!(t.contains("Ana") && !t.contains("id"), "{t}");
        let o = TableOpts { cols: vec!["xx".into()], ..opts() };
        let e = text(&render_table_data("id,nome\n1,Ana\n", &o, 40));
        assert!(e.contains("'xx'") && e.contains("id") && e.contains("nome"), "{e}");
    }

    #[test]
    fn sem_cabecalho_nao_desenha_separador() {
        let o = TableOpts { no_header: true, ..opts() };
        let t = text(&render_table_data("1,2\n3,4\n", &o, 40));
        assert!(!t.contains('├'), "{t}");
        assert!(t.contains('1') && t.contains('3'));
    }

    #[test]
    fn linhas_irregulares_e_aspas_funcionam() {
        let t = text(&render_table_data("a,b\n\"x, y\",1\nso-uma\n", &opts(), 60));
        assert!(t.contains("x, y") && t.contains("so-uma"), "{t}");
    }

    #[test]
    fn coluna_numerica_alinha_a_direita() {
        let r = render_table_data("n\n1\n100\n", &opts(), 40);
        let t = text(&r);
        let linha_1 = t.lines().find(|l| l.contains("│   1")).unwrap_or("");
        assert!(!linha_1.is_empty(), "{t}");
    }

    #[test]
    fn bom_e_removido() {
        let t = text(&render_table_data("\u{feff}id\n1\n", &opts(), 40));
        assert!(t.contains("id") && !t.contains('\u{feff}'), "{t}");
    }

    #[test]
    fn escape_em_celula_nao_vaza() {
        let r = render_table_data("a\n\u{1b}]0;pwn\u{7}\n", &opts(), 40);
        assert!(!crate::render::to_ansi(&r.lines, false).contains('\x1b'));
    }

    #[test]
    fn tabela_larga_e_rapida() {
        let header = (0..500).map(|i| format!("c{i}")).collect::<Vec<_>>().join(",");
        let row = (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let csv = format!("{header}\n{row}\n");
        let t0 = std::time::Instant::now();
        let _ = render_table_data(&csv, &opts(), 100);
        assert!(t0.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn json_aninhado_nao_explode_na_saida_indentada() {
        // entrada pequena, mas cada item ganha ~120 bytes de indentação ao ser formatado
        let json = format!("{}{}{}", "[".repeat(60), "1,".repeat(100_000) + "1", "]".repeat(60));
        assert!(json.len() < MAX_DATA_BYTES);
        let t0 = std::time::Instant::now();
        let t = text(&render_json(&json, None, 60));
        assert!(t.contains("grande"), "{}", &t[..t.len().min(200)]);
        assert!(t0.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn entrada_grande_e_recusada() {
        let big = format!("[{}]", "1,".repeat(MAX_DATA_BYTES));
        assert!(text(&render_json(&big, None, 60)).contains("grande"));
    }
}
