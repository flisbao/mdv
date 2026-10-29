//! Visualização de dados estruturados (JSON/JSONL, CSV/TSV) reaproveitando o renderizador.

use pulldown_cmark::Alignment;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::json_syntax;
use crate::mem;
use crate::render::{error_rendered, memory_error, render_code_only, render_table_only, Rendered, DIM};

use std::collections::{HashMap, HashSet};

use crate::cli::JsonView;
use crate::syntax_error::render_with_error;

pub const MAX_DATA_BYTES: usize = 5 * 1024 * 1024;

/// Lê JSON ou JSONL; o erro vira uma linha só com a posição (quem mostra o texto usa `json_syntax::parse`).
pub fn parse_json(text: &str) -> Result<Value, String> {
    json_syntax::parse(text).map_err(|e| format!("JSON inválido: {} (linha {}, coluna {})", e.message, e.line, e.col))
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

/// Opções do `mdv json`: trecho (`--path`), vista (`--as`) e, na tabela, `--head` e `--cols`.
pub struct JsonOpts {
    pub path: Option<String>,
    pub view: JsonView,
    pub head: usize,
    pub cols: Vec<String>,
}

/// Achatamento de objetos aninhados em colunas `a.b.c`; abaixo disto o valor vira texto compacto.
const MAX_FLATTEN_DEPTH: usize = 4;

enum Shape {
    Records,
    Pairs,
    Scalars,
    Other,
}

fn shape(v: &Value) -> Shape {
    match v {
        Value::Array(a) if !a.is_empty() && a.iter().all(Value::is_object) => Shape::Records,
        Value::Array(a) if !a.is_empty() && a.iter().all(|x| !x.is_object() && !x.is_array()) => {
            Shape::Scalars
        }
        Value::Object(m) if !m.is_empty() => Shape::Pairs,
        _ => Shape::Other,
    }
}

/// Lista de objetos (e JSONL) vira tabela; o resto fica como texto.
pub fn auto_view(v: &Value) -> JsonView {
    if matches!(shape(v), Shape::Records) {
        JsonView::Table
    } else {
        JsonView::Text
    }
}

/// A vista que `Auto` escolheria para este texto (usada pela tecla `v`). `Text` se não parsear.
pub fn auto_view_of_text(text: &str, path: Option<&str>) -> JsonView {
    let Ok(root) = parse_json(text.trim_start_matches('\u{feff}')) else {
        return JsonView::Text;
    };
    match select(&root, path.unwrap_or("")) {
        Ok(sel) => auto_view(sel),
        Err(_) => JsonView::Text,
    }
}

/// Próxima vista no ciclo da tecla `v`: tabela, esquema, texto.
pub fn next_view(current: JsonView) -> JsonView {
    match current {
        JsonView::Auto | JsonView::Text => JsonView::Table,
        JsonView::Table => JsonView::Schema,
        JsonView::Schema => JsonView::Text,
    }
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => clip_cell(s),
        other => clip_cell(&other.to_string()),
    }
}

/// Teto de caminhos de chave distintos guardados ao achatar. Cada caminho custa no máximo
/// `MAX_CELL_CHARS` caracteres, então a memória das chaves fica abaixo de uns 10 MiB.
const MAX_KEY_NODES: usize = 10_000;

/// Pai dos caminhos de primeiro nível.
const KEY_ROOT: usize = usize::MAX;

/// Caminhos de chave (`a.b.c`) vistos ao achatar, guardados como árvore: cada nó é (pai, chave).
/// Assim o custo de um par não depende do tamanho do prefixo (uma chave de 2 MB com 200 mil
/// filhos não vira 200 mil cópias dela) e chaves longas que só diferem no fim continuam distintas.
#[derive(Default)]
struct KeyTree {
    ids: HashMap<(usize, String), usize>,
    /// Nome exibido de cada nó, já cortado em `MAX_CELL_CHARS`.
    names: Vec<String>,
}

impl KeyTree {
    /// Nó da chave `key` sob `parent`; `None` quando o teto de nós foi atingido.
    fn child(&mut self, parent: usize, key: &str) -> Option<usize> {
        if let Some(&id) = self.ids.get(&(parent, key.to_string())) {
            return Some(id);
        }
        if self.names.len() >= MAX_KEY_NODES {
            return None;
        }
        let own = clip_cell(key);
        let name = match self.names.get(parent) {
            Some(prefix) => clip_cell(&format!("{prefix}.{own}")),
            None => own,
        };
        let id = self.names.len();
        self.names.push(name);
        self.ids.insert((parent, key.to_string()), id);
        Some(id)
    }
}

/// Achata objetos aninhados em pares (caminho, texto da célula). Chaves além do teto de nós
/// ficam de fora.
fn flatten(tree: &mut KeyTree, parent: usize, v: &Value, depth: usize, out: &mut Vec<(usize, String)>) {
    match v {
        Value::Object(m) if depth < MAX_FLATTEN_DEPTH && !m.is_empty() => {
            for (k, x) in m {
                if let Some(id) = tree.child(parent, k) {
                    flatten(tree, id, x, depth + 1, out);
                }
            }
        }
        other => {
            let node = if parent == KEY_ROOT { tree.child(KEY_ROOT, "") } else { Some(parent) };
            if let Some(id) = node {
                out.push((id, cell_text(other)));
            }
        }
    }
}

/// Uma linha por objeto, uma coluna por chave (na ordem em que aparecem), até `MAX_TABLE_COLS`.
/// Devolve as linhas (com o cabeçalho primeiro) e quantas colunas ficaram de fora.
fn records_to_rows(items: &[Value], head: usize) -> (Vec<Vec<String>>, usize) {
    let mut tree = KeyTree::default();
    let flat: Vec<Vec<(usize, String)>> = items
        .iter()
        .take(head)
        .take_while(|_| !mem::exceeded())
        .map(|item| {
            let mut pairs = Vec::new();
            flatten(&mut tree, KEY_ROOT, item, 0, &mut pairs);
            pairs
        })
        .collect();
    let mut column: HashMap<usize, usize> = HashMap::new();
    let mut keys: Vec<usize> = Vec::new();
    let mut omitted: HashSet<usize> = HashSet::new();
    for pairs in &flat {
        for &(node, _) in pairs {
            if column.contains_key(&node) || omitted.contains(&node) {
                continue;
            }
            if keys.len() >= MAX_TABLE_COLS {
                omitted.insert(node);
            } else {
                column.insert(node, keys.len());
                keys.push(node);
            }
        }
    }
    let mut rows = vec![keys.iter().map(|&n| tree.names[n].clone()).collect::<Vec<_>>()];
    for pairs in &flat {
        let mut row = vec![String::new(); keys.len()];
        for (node, v) in pairs {
            if let Some(&i) = column.get(node) {
                row[i].clone_from(v);
            }
        }
        rows.push(row);
    }
    (rows, omitted.len())
}

pub(crate) fn render_json_table(sel: &Value, o: &JsonOpts, width: usize) -> Rendered {
    let (rows, total, omitted) = match (shape(sel), sel) {
        (Shape::Records, Value::Array(items)) => {
            let (rows, omitted) = records_to_rows(items, o.head);
            (rows, items.len(), omitted)
        }
        (Shape::Pairs, _) => {
            let mut tree = KeyTree::default();
            let mut pairs = Vec::new();
            flatten(&mut tree, KEY_ROOT, sel, 0, &mut pairs);
            let total = pairs.len();
            let mut rows = vec![vec!["chave".to_string(), "valor".to_string()]];
            rows.extend(pairs.into_iter().take(o.head).map(|(k, v)| vec![tree.names[k].clone(), v]));
            (rows, total, 0)
        }
        (Shape::Scalars, Value::Array(items)) => {
            let mut rows = vec![vec!["valor".to_string()]];
            rows.extend(items.iter().take(o.head).map(|x| vec![cell_text(x)]));
            (rows, items.len(), 0)
        }
        _ => {
            return error_rendered("esta estrutura não cabe em tabela (use --as text ou --as schema)");
        }
    };
    finish_table(rows, true, total, omitted, &o.cols, width, "itens")
}

pub(crate) fn render_json_text(sel: &Value, width: usize) -> Rendered {
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

const MAX_SCHEMA_DEPTH: usize = 8;
const MAX_SCHEMA_KEYS: usize = 200;
/// Itens analisados por lista: o esquema de um milhão de registros sai de uma amostra.
const MAX_SCHEMA_SAMPLE: usize = 1000;
const MAX_SCHEMA_LINES: usize = 2000;

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Object(_) => "objeto",
        Value::Array(_) => "lista",
        Value::String(_) => "texto",
        Value::Number(_) => "número",
        Value::Bool(_) => "booleano",
        Value::Null => "nulo",
    }
}

/// Descreve os valores que ocupam o mesmo lugar no documento. Devolve o rótulo do tipo e
/// acrescenta a `lines` as linhas dos filhos (indentadas a partir de `depth + 1`).
fn group(values: &[&Value], depth: usize, lines: &mut Vec<String>) -> String {
    let mut kinds: Vec<&'static str> = Vec::new();
    for v in values {
        let k = kind(v);
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    if depth >= MAX_SCHEMA_DEPTH {
        return format!("{} …", kinds.join(" | "));
    }

    let objects: Vec<&serde_json::Map<String, Value>> =
        values.iter().filter_map(|v| v.as_object()).take(MAX_SCHEMA_SAMPLE).collect();
    if !objects.is_empty() {
        let mut keys: Vec<&str> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut cut = false;
        for m in &objects {
            for k in m.keys() {
                if seen.contains(k.as_str()) {
                    continue;
                }
                if keys.len() >= MAX_SCHEMA_KEYS {
                    cut = true;
                    continue;
                }
                seen.insert(k);
                keys.push(k);
            }
        }
        for key in keys {
            let vals: Vec<&Value> = objects.iter().filter_map(|m| m.get(key)).collect();
            let optional = if vals.len() < objects.len() { "?" } else { "" };
            let at = lines.len();
            lines.push(String::new());
            let label = group(&vals, depth + 1, lines);
            lines[at] = format!("{}{key}{optional}: {label}", "  ".repeat(depth + 1));
        }
        if cut {
            lines.push(format!("{}…", "  ".repeat(depth + 1)));
        }
    }

    let mut parts: Vec<String> = Vec::new();
    for k in kinds {
        if k != "lista" {
            parts.push(k.to_string());
            continue;
        }
        let total: usize = values.iter().filter_map(|v| v.as_array()).map(Vec::len).sum();
        if total == 0 {
            parts.push("lista · vazia".to_string());
            continue;
        }
        let elems: Vec<&Value> =
            values.iter().filter_map(|v| v.as_array()).flatten().take(MAX_SCHEMA_SAMPLE).collect();
        let label = group(&elems, depth, lines);
        parts.push(format!("lista · {total} × {label}"));
    }
    parts.join(" | ")
}

/// Tipos do documento, uma linha por campo, indentado. Listas são resumidas por uma amostra.
pub fn schema_text(v: &Value) -> String {
    let mut lines = vec![String::new()];
    lines[0] = group(&[v], 0, &mut lines);
    if lines.len() > MAX_SCHEMA_LINES {
        lines.truncate(MAX_SCHEMA_LINES);
        lines.push("…".to_string());
    }
    lines.join("\n")
}

pub(crate) fn render_json_schema(sel: &Value, width: usize) -> Rendered {
    render_code_only(&schema_text(sel), "", width)
}

pub fn render_json(text: &str, o: &JsonOpts, width: usize) -> Rendered {
    if text.len() > MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo JSON (limite de 5 MiB)");
    }
    if let Err(e) = mem::check_json(text.len()) {
        return error_rendered(&e.to_string());
    }
    let text = text.trim_start_matches('\u{feff}');
    let root = match json_syntax::parse(text) {
        Ok(v) => v,
        Err(e) => return render_with_error(text, "json", width, &e),
    };
    if let Some(err) = memory_error() {
        return err;
    }
    let sel = match select(&root, o.path.as_deref().unwrap_or("")) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let view = if o.view == JsonView::Auto { auto_view(sel) } else { o.view };
    let out = match view {
        JsonView::Table => render_json_table(sel, o, width),
        JsonView::Schema => render_json_schema(sel, width),
        JsonView::Text | JsonView::Auto => render_json_text(sel, width),
    };
    memory_error().unwrap_or(out)
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

/// Teto de colunas de uma tabela; o resto é só contado (`+N omitidas`).
pub const MAX_TABLE_COLS: usize = 200;
/// Teto de caracteres por célula; o excesso vira `…`.
pub const MAX_CELL_CHARS: usize = 500;

fn clip_cell(s: &str) -> String {
    if s.chars().count() <= MAX_CELL_CHARS {
        return s.to_string();
    }
    let mut cut: String = s.chars().take(MAX_CELL_CHARS).collect();
    cut.push('…');
    cut
}

/// Fim comum de CSV/TSV e JSON em tabela: seleção de colunas (`--cols`), alinhamento
/// numérico, desenho e rodapé. `rows[0]` é o cabeçalho quando `has_header`.
fn finish_table(
    mut rows: Vec<Vec<String>>,
    has_header: bool,
    data_total: usize,
    omitted_cols: usize,
    cols: &[String],
    width: usize,
    unit: &str,
) -> Rendered {
    if !cols.is_empty() {
        if !has_header {
            return error_rendered("--cols precisa de cabeçalho (remova --no-header)");
        }
        let header = rows[0].clone();
        let mut idx = Vec::new();
        for name in cols {
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
    let omitted = if omitted_cols > 0 { format!(" (+{omitted_cols} omitidas)") } else { String::new() };
    r.lines.push(Line::from(Span::styled(
        format!("{shown} de {data_total} {unit} · {ncols} colunas{omitted}"),
        Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
    )));
    r
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
    let (mut rows, mut total, mut omitted_cols): (Vec<Vec<String>>, usize, usize) = (Vec::new(), 0, 0);
    for rec in rdr.records() {
        if let Some(err) = memory_error() {
            return err;
        }
        let rec = match rec {
            Ok(r) => r,
            Err(e) => return error_rendered(&format!("CSV inválido: {e}")),
        };
        total += 1;
        omitted_cols = omitted_cols.max(rec.len().saturating_sub(MAX_TABLE_COLS));
        if rows.len() < keep {
            rows.push(rec.iter().take(MAX_TABLE_COLS).map(clip_cell).collect());
        }
    }
    if rows.is_empty() {
        return error_rendered("arquivo vazio");
    }
    let has_header = !o.no_header;
    let data_total = total - usize::from(has_header);
    let out = finish_table(rows, has_header, data_total, omitted_cols, &o.cols, width, "linhas");
    memory_error().unwrap_or(out)
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
        assert!(e.contains("linha 2") && e.contains("coluna 8"), "{e}");
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
        let t = text(&render_json("{\"nome\":\"mdv\",\"v\":[1,2]}", &jopts(JsonView::Text), 60));
        assert!(t.contains("objeto · 2 chaves"), "{t}");
        assert!(t.contains("\"nome\": \"mdv\""), "{t}");
    }

    #[test]
    fn render_json_com_caminho_mostra_so_o_trecho() {
        let t = text(&render_json("{\"a\":{\"b\":1},\"c\":2}", &JsonOpts { path: Some(".a".into()), ..jopts(JsonView::Text) }, 60));
        assert!(t.contains("\"b\": 1") && !t.contains("\"c\""), "{t}");
    }

    #[test]
    fn render_json_erro_mostra_o_texto_com_a_mensagem() {
        let r = render_json("{", &jopts(JsonView::Text), 60);
        let t = text(&r);
        assert!(t.contains("✗ linha 1, coluna 1"), "{t}");
        assert!(t.contains("fim inesperado"), "{t}");
    }

    #[test]
    fn json_invalido_mostra_o_texto_com_a_linha_do_erro() {
        let r = render_json("{\n\"a\": 1,\n}\n", &jopts(JsonView::Text), 60);
        let t = text(&r);
        assert!(t.contains('{') && t.contains("\"a\": 1,"), "{t}");
        assert!(t.contains("✗ linha 3"), "{t}");
        assert!(r.focus.is_some());
    }

    #[test]
    fn json_invalido_com_bom_nao_desloca_o_erro() {
        let t = text(&render_json("\u{feff}{\n\"a\": 1,\n}\n", &jopts(JsonView::Text), 60));
        assert!(t.contains("✗ linha 3"), "{t}");
    }

    #[test]
    fn controle_em_string_nao_vaza_para_o_terminal() {
        let r = render_json("{\"x\":\"\\u001b]0;pwn\\u0007\"}", &jopts(JsonView::Text), 60);
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
        let t = text(&render_table_data("1,2\n", &o, 40));
        assert!(!t.contains('├'), "{t}");
        assert!(t.contains('1') && t.contains('2'));
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
        let t = text(&render_json(&json, &jopts(JsonView::Text), 60));
        assert!(t.contains("grande"), "{}", &t[..t.len().min(200)]);
        assert!(t0.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn entrada_grande_e_recusada() {
        let big = format!("[{}]", "1,".repeat(MAX_DATA_BYTES));
        assert!(text(&render_json(&big, &jopts(JsonView::Text), 60)).contains("grande"));
    }
        #[test]
        fn celula_gigante_e_cortada_com_reticencias() {
            assert_eq!(clip_cell("curto"), "curto");
            let cortada = clip_cell(&"é".repeat(MAX_CELL_CHARS + 50));
            assert_eq!(cortada.chars().count(), MAX_CELL_CHARS + 1);
            assert!(cortada.ends_with('…'));
        }

        #[test]
        fn csv_com_milhares_de_colunas_e_limitado() {
            let header = (0..5000).map(|i| format!("c{i}")).collect::<Vec<_>>().join(",");
            let t = text(&render_table_data(&format!("{header}\n1\n"), &opts(), 80));
            assert!(t.contains("200 colunas") && t.contains("+4800 omitidas"), "{t}");
        }

        #[test]
        fn linha_de_csv_com_milhoes_de_colunas_e_rapida() {
            let csv = format!("{}\n", ",".repeat(2_000_000));
            let t0 = std::time::Instant::now();
            let t = text(&render_table_data(&csv, &opts(), 80));
            assert!(t.contains("omitidas"), "{}", &t[..t.len().min(200)]);
            assert!(t0.elapsed() < std::time::Duration::from_secs(3), "levou {:?}", t0.elapsed());
        }

        use crate::cli::JsonView;

        fn jopts(view: JsonView) -> JsonOpts {
            JsonOpts { path: None, view, head: 1000, cols: vec![] }
        }

        #[test]
        fn lista_de_objetos_vira_tabela_com_uniao_de_chaves() {
            let json = r#"[{"id":1,"nome":"Ana"},{"id":2,"extra":true}]"#;
            let t = text(&render_json(json, &jopts(JsonView::Auto), 80));
            for s in ["id", "nome", "extra", "Ana", "true"] {
                assert!(t.contains(s), "falta {s}:\n{t}");
            }
            assert!(t.contains("2 de 2 itens") && t.contains("3 colunas"), "{t}");
        }

        #[test]
        fn objetos_aninhados_viram_colunas_com_ponto() {
            let t = text(&render_json(r#"[{"a":{"b":1,"c":{"d":2}}}]"#, &jopts(JsonView::Table), 80));
            assert!(t.contains("a.b") && t.contains("a.c.d"), "{t}");
        }

        #[test]
        fn listas_dentro_da_celula_ficam_compactas() {
            let t = text(&render_json(r#"[{"tags":["x","y"]}]"#, &jopts(JsonView::Table), 80));
            assert!(t.contains(r#"["x","y"]"#), "{t}");
        }

        #[test]
        fn objeto_unico_vira_chave_valor_na_vista_tabela() {
            let t = text(&render_json(r#"{"nome":"mdv","v":{"n":1}}"#, &jopts(JsonView::Table), 80));
            assert!(t.contains("chave") && t.contains("valor") && t.contains("v.n"), "{t}");
        }

        #[test]
        fn lista_de_escalares_vira_coluna_valor() {
            let t = text(&render_json("[1,2,3]", &jopts(JsonView::Table), 80));
            assert!(t.contains("valor") && t.contains("3 de 3 itens"), "{t}");
        }

        #[test]
        fn jsonl_vira_tabela_no_automatico() {
            let t = text(&render_json("{\"a\":1}\n{\"a\":2}\n", &jopts(JsonView::Auto), 80));
            assert!(t.contains("2 de 2 itens"), "{t}");
        }

        #[test]
        fn automatico_mantem_texto_para_o_que_nao_e_lista_de_objetos() {
            let t = text(&render_json(r#"{"a":1}"#, &jopts(JsonView::Auto), 80));
            assert!(t.contains("objeto · 1 chaves"), "{t}");
        }

        #[test]
        fn head_e_cols_valem_na_tabela_json() {
            let json = r#"[{"id":1,"nome":"A"},{"id":2,"nome":"B"},{"id":3,"nome":"C"}]"#;
            let o = JsonOpts { head: 1, ..jopts(JsonView::Table) };
            assert!(text(&render_json(json, &o, 80)).contains("1 de 3 itens"));
            let o = JsonOpts { cols: vec!["id".into()], ..jopts(JsonView::Table) };
            let t = text(&render_json(json, &o, 80));
            assert!(!t.contains("nome"), "{t}");
        }

        #[test]
        fn estrutura_que_nao_cabe_em_tabela_da_mensagem() {
            for json in ["[[1],[2]]", r#"[1,{"a":1}]"#, "[]", "5"] {
                let t = text(&render_json(json, &jopts(JsonView::Table), 80));
                assert!(t.contains("não cabe em tabela"), "{json}: {t}");
            }
        }

        #[test]
        fn json_largo_tem_teto_de_colunas_e_e_rapido() {
            let obj = (0..5000).map(|i| format!("\"k{i}\":{i}")).collect::<Vec<_>>().join(",");
            let t0 = std::time::Instant::now();
            let t = text(&render_json(&format!("[{{{obj}}}]"), &jopts(JsonView::Table), 80));
            assert!(t.contains("200 colunas") && t.contains("+4800 omitidas"), "{t}");
            assert!(t0.elapsed() < std::time::Duration::from_secs(3));
        }

        #[test]
        fn aninhamento_muito_fundo_para_no_nivel_quatro() {
            let json = format!("[{}1{}]", "{\"a\":".repeat(50), "}".repeat(50));
            let t = text(&render_json(&json, &jopts(JsonView::Table), 200));
            assert!(t.contains("a.a.a.a"), "{t}");
            assert!(!t.contains("a.a.a.a.a"), "{t}");
        }

        #[test]
        fn controle_em_chave_e_valor_nao_vaza_na_tabela() {
            let r = render_json(r#"[{"\u001b]0;pwn\u0007":"\u001b[31mx"}]"#, &jopts(JsonView::Table), 80);
            assert!(!crate::render::to_ansi(&r.lines, false).contains('\x1b'));
        }

        #[test]
        fn caminho_vale_antes_da_vista() {
            let o = JsonOpts { path: Some(".d".into()), ..jopts(JsonView::Auto) };
            let t = text(&render_json(r#"{"d":[{"a":1}],"z":9}"#, &o, 80));
            assert!(t.contains("1 de 1 itens") && !t.contains('z'), "{t}");
        }

        #[test]
        fn vista_automatica_de_texto_bruto() {
            assert_eq!(auto_view_of_text(r#"[{"a":1}]"#, None), JsonView::Table);
            assert_eq!(auto_view_of_text(r#"{"a":1}"#, None), JsonView::Text);
            assert_eq!(auto_view_of_text("{", None), JsonView::Text);
            assert_eq!(auto_view_of_text(r#"{"d":[{"a":1}]}"#, Some(".d")), JsonView::Table);
        }

        fn schema(json: &str) -> String {
            schema_text(&parse_json(json).unwrap())
        }

        #[test]
        fn esquema_de_objeto_com_lista_de_objetos() {
            let s = schema(r#"{"nome":"mdv","itens":[{"id":1,"tags":["a"]},{"id":2}]}"#);
            assert_eq!(
                s,
                "objeto\n  nome: texto\n  itens: lista · 2 × objeto\n    id: número\n    tags?: lista · 1 × texto"
            );
        }

        #[test]
        fn esquema_marca_tipos_mistos_e_lista_vazia() {
            assert_eq!(schema(r#"[{"a":1},{"a":"x"}]"#), "lista · 2 × objeto\n  a: número | texto");
            assert_eq!(schema("[]"), "lista · vazia");
            assert_eq!(schema("null"), "nulo");
        }

        #[test]
        fn esquema_para_na_profundidade_maxima() {
            let json = format!("{}1{}", "{\"a\":".repeat(30), "}".repeat(30));
            let s = schema(&json);
            assert!(s.contains('…'), "{s}");
            assert!(s.lines().count() <= MAX_SCHEMA_DEPTH + 2, "{s}");
        }

        #[test]
        fn esquema_limita_chaves_e_linhas() {
            let obj = (0..5000).map(|i| format!("\"k{i}\":{i}")).collect::<Vec<_>>().join(",");
            let s = schema(&format!("{{{obj}}}"));
            assert!(s.lines().count() <= MAX_SCHEMA_KEYS + 3, "{} linhas", s.lines().count());
            assert!(s.contains('…'));
        }

        #[test]
        fn esquema_de_documento_grande_e_rapido() {
            let items = (0..200_000).map(|i| format!("{{\"id\":{i},\"nome\":\"n{i}\"}}")).collect::<Vec<_>>().join(",");
            let t0 = std::time::Instant::now();
            let s = schema(&format!("[{items}]"));
            assert!(s.starts_with("lista · 200000 × objeto"), "{s}");
            assert!(t0.elapsed() < std::time::Duration::from_secs(3), "levou {:?}", t0.elapsed());
        }

        #[test]
        fn esquema_pela_vista_schema_e_sem_escape() {
            let r = render_json(r#"{"\u001b]0;pwn\u0007":1}"#, &jopts(JsonView::Schema), 80);
            let ansi = crate::render::to_ansi(&r.lines, false);
            assert!(!ansi.contains('\x1b') && ansi.contains("número"), "{ansi:?}");
        }

        #[test]
        fn ciclo_de_vistas() {
            assert_eq!(next_view(JsonView::Auto), JsonView::Table);
            assert_eq!(next_view(JsonView::Text), JsonView::Table);
            assert_eq!(next_view(JsonView::Table), JsonView::Schema);
            assert_eq!(next_view(JsonView::Schema), JsonView::Text);
        }

    #[test]
    fn chave_longa_com_muitos_filhos_nao_explode_a_memoria() {
        let filhos = (0..200_000).map(|i| format!("\"k{i}\":0")).collect::<Vec<_>>().join(",");
        let json = format!("[{{\"{}\":{{{filhos}}}}}]", "A".repeat(2_000_000));
        let t0 = std::time::Instant::now();
        let t = text(&render_json(&json, &jopts(JsonView::Table), 80));
        assert!(t.contains("omitidas"), "{}", &t[..t.len().min(300)]);
        assert!(t0.elapsed() < std::time::Duration::from_secs(3), "levou {:?}", t0.elapsed());
    }
}
