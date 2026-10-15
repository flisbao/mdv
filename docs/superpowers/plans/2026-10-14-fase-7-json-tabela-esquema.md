# F7.6 — JSON em tabela e esquema: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fazer o `mdv json` mostrar o JSON também como **tabela** e como **esquema** (chaves e tipos), com `--as <vista>` e a tecla `v` na TUI, e pôr teto de colunas/células nas tabelas (CSV inclusive).

**Architecture:** `src/data.rs` ganha três vistas sobre o mesmo `serde_json::Value` já selecionado por `--path`: `Text` (a atual), `Table` (valor → linhas de strings → o mesmo desenho de tabela do CSV, via `render_table_only`) e `Schema` (texto indentado de tipos, mostrado com `render_code_only`). O fim do fluxo do CSV (`--cols`, alinhamento numérico, rodapé) vira uma função comum `finish_table`. `src/cli.rs` carrega a vista escolhida (`JsonView`) e `src/main.rs` a guarda no `DocOpts` e a alterna com `v`.

**Tech Stack:** Rust 2021, `serde_json` (já usa `preserve_order`), `ratatui`, `pulldown-cmark::Alignment`. Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`, seção "F7.6 JSON em tabela e esquema"; item F7.6 do `PLANO.md`. Este item não depende da fundação da Fase 7 (`line_src`, `code_blocks`, `tasks`) nem de F7.1–F7.5, e pode ser feito antes ou depois deles.

## Global Constraints

- Mensagens de erro, ajuda e README em português, com acentuação correta.
- Esta fase não acrescenta I/O: sem rede, sem subprocesso, sem dependência nova.
- Entrada de dados continua limitada a `MAX_DATA_BYTES = 5 MiB` e a saída indentada a `MAX_PRETTY_BYTES = 8 MiB` (já existem em `src/data.rs`).
- Todo texto de chave ou valor que chega ao terminal passa por `render::sanitize` (as vistas usam `render_table_only` e `render_code_only`, que já sanitizam; não escreva `Span` direto).
- Saída de `--plain` nunca contém `\x1b` além do SGR gerado pelo `mdv`.
- Tetos fixos desta feature: tabela com no máximo `MAX_TABLE_COLS = 200` colunas; célula com no máximo `MAX_CELL_CHARS = 500` caracteres; achatamento de objetos até `MAX_FLATTEN_DEPTH = 4` níveis; esquema com `MAX_SCHEMA_DEPTH = 8` níveis, `MAX_SCHEMA_KEYS = 200` chaves por objeto, `MAX_SCHEMA_SAMPLE = 1000` itens analisados e `MAX_SCHEMA_LINES = 2000` linhas.
- `cargo clippy -- -W clippy::pedantic` sem warnings; sem `unsafe`.
- Testes não usam rede nem subprocesso.

## Review Focus

1. **JSON com milhares de chaves distintas** (um objeto largo, ou registros com chaves diferentes entre si): a tabela não pode virar milhares de colunas nem ficar lenta; deve mostrar 200 e avisar `+N omitidas` (Tasks 2 e 3).
2. **Células e aninhamento gigantes**: um valor de 1 MiB numa célula, ou objetos aninhados em 100 níveis, não podem travar nem estourar a memória; a célula é cortada com `…` e o achatamento para no nível 4 (Tasks 2 e 3).
3. **Caracteres de controle em chaves e valores** (`"\u001b]0;x\u0007"` como chave ou valor, na tabela e no esquema): nada de `\x1b` na saída (Tasks 3 e 4).
4. **Esquema de documento grande**: milhões de itens ou milhares de chaves não podem custar tempo quadrático nem gerar milhares de linhas; vale amostra de 1000 itens e teto de linhas (Task 4).
5. **Uso fora do JSON**: `--as` com Markdown/CSV é erro claro; `v` fora do modo JSON não faz nada; estrutura que não cabe em tabela (lista de listas, lista mista) dá mensagem e não pânico (Tasks 1, 3 e 5).

---

## File Structure

| Arquivo | Mudança |
|---|---|
| `src/cli.rs` | `JsonView`, campo `Cli.json_view`, flag `--as`, validação |
| `src/data.rs` | `finish_table` (comum a CSV e JSON), tetos, `JsonOpts`, vistas `Table` e `Schema`, `render_json(text, &JsonOpts, width)`, `auto_view_of_text`, `next_view` |
| `src/main.rs` | `DocOpts.json_view`, tecla `v`, rótulo na barra de status, `HELP` |
| `tests/cli.rs` | testes de integração de `--as` |
| `samples/itens.json`, `samples/eventos.jsonl` | amostras |
| `README.md`, `CLAUDE.md`, `docs/pendencias.md` | documentação |

## Pontos do código atual que o plano usa

- `src/data.rs`: `render_json(text: &str, path: Option<&str>, width: usize) -> Rendered`, `render_table_data(text, &TableOpts, width)`, `TableOpts { delimiter, head, cols, no_header }`, `is_numeric_column`, `select`, `parse_json`, `describe`, `pretty_bounded`, `MAX_DATA_BYTES`. Os imports são `Alignment`, `Modifier`, `Style`, `Line`, `Span`, `Value`, e `crate::render::{error_rendered, render_code_only, render_table_only, Rendered, DIM}`.
- `src/render.rs`: `render_table_only(rows: Vec<Vec<String>>, header: bool, aligns: Vec<Alignment>, width: usize) -> Rendered`, `render_code_only(code, lang, width)`, `error_rendered(msg)`. Ambos sanitizam o texto.
- `src/main.rs`: `DocOpts { mode, json_path, head, cols, no_header }` com `from_cli`, `render_doc(&DocOpts, text, width, &ImageStore)`, `App.doc: DocOpts`, `App::on_key`, `App::status_left`, `HELP`, e no módulo de testes `test_app(source, md) -> App` (sempre com o argumento `x.md`).
- `src/cli.rs`: `parse`, `Cli`, `Mode`, `value(&mut args, flag)`, e no módulo de testes `run(&[&str]) -> Result<Cli, String>`.

Comando de teste do projeto: `cargo test` (unidades de `src/` e `tests/cli.rs`).

---

### Task 1: Flag `--as` e `JsonView` no CLI

**Files:**
- Modify: `src/cli.rs`
- Test: `src/cli.rs` (módulo `tests`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum JsonView { Auto, Text, Table, Schema }
  // Cli ganha: pub json_view: JsonView   (Auto por padrão)
  ```

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/cli.rs`, ao lado dos outros testes:

```rust
    #[test]
    fn as_define_a_vista_do_json() {
        assert_eq!(run(&["json", "d.json", "--as", "table"]).unwrap().json_view, JsonView::Table);
        assert_eq!(run(&["json", "d.json", "--as", "schema"]).unwrap().json_view, JsonView::Schema);
        assert_eq!(run(&["json", "d.json", "--as", "text"]).unwrap().json_view, JsonView::Text);
        assert_eq!(run(&["json", "d.json", "--as", "auto"]).unwrap().json_view, JsonView::Auto);
        assert_eq!(run(&["d.json"]).unwrap().json_view, JsonView::Auto);
    }

    #[test]
    fn as_por_extensao_tambem_vale() {
        assert_eq!(run(&["d.jsonl", "--as", "table"]).unwrap().json_view, JsonView::Table);
    }

    #[test]
    fn as_invalido_ou_fora_do_json_e_erro() {
        assert!(run(&["json", "--as", "xml", "-"]).unwrap_err().contains("xml"));
        assert!(run(&["--as"]).is_err());
        assert!(run(&["a.md", "--as", "table"]).unwrap_err().contains("--as"));
        assert!(run(&["csv", "a.csv", "--as", "table"]).unwrap_err().contains("--as"));
    }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test cli::tests::as_`
Expected: erro de compilação (`JsonView` e `json_view` não existem).

- [ ] **Step 3: Implementar**

Em `src/cli.rs`, depois do `enum Mode`:

```rust
/// Como o JSON é mostrado. `Auto` vira tabela para lista de objetos (e JSONL) e texto nos demais casos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonView {
    Auto,
    Text,
    Table,
    Schema,
}
```

Em `struct Cli`, depois de `pub json_path: Option<String>,`:

```rust
    pub json_view: JsonView,
```

No literal `Cli { .. }` de `parse`, depois de `json_path: None,`:

```rust
        json_view: JsonView::Auto,
```

No `match a.as_str()` de `parse`, depois do braço `"--path"`:

```rust
            "--as" => {
                let v = value(&mut args, &a)?;
                cli.json_view = match v.as_str() {
                    "auto" => JsonView::Auto,
                    "text" => JsonView::Text,
                    "table" => JsonView::Table,
                    "schema" => JsonView::Schema,
                    _ => return Err(format!("vista inválida: {v} (use auto, text, table ou schema)")),
                };
            }
```

No fim de `parse`, logo antes de `Ok(Parsed::Run(cli))`:

```rust
    if cli.json_view != JsonView::Auto && cli.mode != Mode::Json {
        return Err("a opção --as só vale para JSON".to_string());
    }
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test cli::`
Expected: todos PASS (os 3 novos incluídos).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): opção --as para escolher a vista do JSON"
```

---

### Task 2: Parte comum das tabelas (`finish_table`) com teto de colunas e células

Refatora o fim de `render_table_data` numa função reutilizável pelo JSON e aplica os tetos. Fecha de quebra a pendência "CSV com milhões de colunas em uma linha" de `docs/pendencias.md`.

**Files:**
- Modify: `src/data.rs`
- Test: `src/data.rs` (módulo `tests`)

**Interfaces:**
- Produces:
  ```rust
  pub const MAX_TABLE_COLS: usize = 200;
  pub const MAX_CELL_CHARS: usize = 500;
  fn clip_cell(s: &str) -> String
  fn finish_table(rows: Vec<Vec<String>>, has_header: bool, data_total: usize,
                  omitted_cols: usize, cols: &[String], width: usize, unit: &str) -> Rendered
  ```
  `rows` já inclui a linha de cabeçalho em `rows[0]` quando `has_header`. O rodapé é `"{shown} de {data_total} {unit} · {ncols} colunas"`, mais `" (+{omitted_cols} omitidas)"` quando `omitted_cols > 0`.

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/data.rs` (usa o helper `opts()` e `text()` que já existem lá):

```rust
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
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test data::tests::celula_gigante data::tests::csv_com data::tests::linha_de_csv`
Expected: erro de compilação (`clip_cell`, `MAX_CELL_CHARS` não existem). Depois de declarar só as constantes e `clip_cell` vazios, os dois outros falham por asserção.

- [ ] **Step 3: Implementar**

Em `src/data.rs`, trocar a função `render_table_data` inteira por este bloco (mantém `TableOpts` e `is_numeric_column` como estão):

```rust
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
    finish_table(rows, has_header, data_total, omitted_cols, &o.cols, width, "linhas")
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test data::`
Expected: todos PASS, incluindo os 10 testes de CSV que já existiam (o rodapé `2 de 2 linhas · 2 colunas` não mudou).

- [ ] **Step 5: Commit**

```bash
git add src/data.rs
git commit -m "refactor(dados): finish_table comum e tetos de colunas e células nas tabelas"
```

---

### Task 3: Vista de tabela para JSON (`JsonOpts`, `--as table`, automático para lista de objetos e JSONL)

**Files:**
- Modify: `src/data.rs`, `src/main.rs`
- Test: `src/data.rs` (módulo `tests`), `tests/cli.rs`

**Interfaces:**
- Consumes: `finish_table`, `clip_cell`, `MAX_TABLE_COLS` (Task 2); `crate::cli::JsonView` (Task 1).
- Produces:
  ```rust
  pub struct JsonOpts { pub path: Option<String>, pub view: JsonView, pub head: usize, pub cols: Vec<String> }
  pub fn render_json(text: &str, o: &JsonOpts, width: usize) -> Rendered   // nova assinatura
  pub fn auto_view(v: &Value) -> JsonView                                   // Table | Text
  pub fn auto_view_of_text(text: &str, path: Option<&str>) -> JsonView      // Text se não parsear
  ```
  Em `src/main.rs`, `DocOpts` ganha `json_view: cli::JsonView`.

- [ ] **Step 1: Escrever os testes que falham (unidade)**

No módulo `tests` de `src/data.rs`. Primeiro um helper, depois os testes:

```rust
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
```

Os 8 testes antigos de JSON que chamam `render_json("...", None, 60)` ou `Some(".a")` passam a usar a nova assinatura: troque `None` por `&jopts(JsonView::Text)` e, no teste de caminho, por `&JsonOpts { path: Some(".a".into()), ..jopts(JsonView::Text) }`. São eles: `render_json_mostra_resumo_e_conteudo_indentado`, `render_json_com_caminho_mostra_so_o_trecho`, `render_json_erro_vira_linha_de_erro`, `controle_em_string_nao_vaza_para_o_terminal`, `entrada_grande_e_recusada`, `json_aninhado_nao_explode_na_saida_indentada`. (Os testes de `select`, `parse_json` e `describe` não mudam.)

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test data::`
Expected: erro de compilação (`JsonOpts`, `auto_view_of_text` não existem; `render_json` com assinatura antiga).

- [ ] **Step 3: Implementar em `src/data.rs`**

Imports: acrescentar `use std::collections::{HashMap, HashSet};` e `use crate::cli::JsonView;`.

Antes de `pub fn render_json`, acrescentar:

```rust
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

fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => clip_cell(s),
        other => clip_cell(&other.to_string()),
    }
}

fn flatten(prefix: &str, v: &Value, depth: usize, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(m) if depth < MAX_FLATTEN_DEPTH && !m.is_empty() => {
            for (k, x) in m {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&key, x, depth + 1, out);
            }
        }
        other => out.push((prefix.to_string(), cell_text(other))),
    }
}

/// Uma linha por objeto, uma coluna por chave (na ordem em que aparecem), até `MAX_TABLE_COLS`.
/// Devolve as linhas (com o cabeçalho primeiro) e quantas colunas ficaram de fora.
fn records_to_rows(items: &[Value], head: usize) -> (Vec<Vec<String>>, usize) {
    let flat: Vec<Vec<(String, String)>> = items
        .iter()
        .take(head)
        .map(|item| {
            let mut pairs = Vec::new();
            flatten("", item, 0, &mut pairs);
            pairs
        })
        .collect();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut keys: Vec<&str> = Vec::new();
    let mut omitted: HashSet<&str> = HashSet::new();
    for pairs in &flat {
        for (k, _) in pairs {
            if index.contains_key(k.as_str()) || omitted.contains(k.as_str()) {
                continue;
            }
            if keys.len() >= MAX_TABLE_COLS {
                omitted.insert(k);
            } else {
                index.insert(k, keys.len());
                keys.push(k);
            }
        }
    }
    let mut rows = vec![keys.iter().map(|k| clip_cell(k)).collect::<Vec<_>>()];
    for pairs in &flat {
        let mut row = vec![String::new(); keys.len()];
        for (k, v) in pairs {
            if let Some(&i) = index.get(k.as_str()) {
                row[i].clone_from(v);
            }
        }
        rows.push(row);
    }
    (rows, omitted.len())
}

fn render_json_table(sel: &Value, o: &JsonOpts, width: usize) -> Rendered {
    let (rows, total, omitted) = match (shape(sel), sel) {
        (Shape::Records, Value::Array(items)) => {
            let (rows, omitted) = records_to_rows(items, o.head);
            (rows, items.len(), omitted)
        }
        (Shape::Pairs, _) => {
            let mut pairs = Vec::new();
            flatten("", sel, 0, &mut pairs);
            let total = pairs.len();
            let mut rows = vec![vec!["chave".to_string(), "valor".to_string()]];
            rows.extend(pairs.into_iter().take(o.head).map(|(k, v)| vec![clip_cell(&k), v]));
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

fn render_json_text(sel: &Value, width: usize) -> Rendered {
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
```

Trocar a função `render_json` atual por esta (o corpo de formatação foi para `render_json_text`):

```rust
pub fn render_json(text: &str, o: &JsonOpts, width: usize) -> Rendered {
    if text.len() > MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo JSON (limite de 5 MiB)");
    }
    let root = match parse_json(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let sel = match select(&root, o.path.as_deref().unwrap_or("")) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let view = if o.view == JsonView::Auto { auto_view(sel) } else { o.view };
    match view {
        JsonView::Table => render_json_table(sel, o, width),
        JsonView::Schema => error_rendered("vista schema ainda não disponível"),
        JsonView::Text | JsonView::Auto => render_json_text(sel, width),
    }
}
```

(O braço `Schema` provisório é substituído na Task 4.)

- [ ] **Step 4: Ligar no `src/main.rs`**

Em `struct DocOpts`, depois de `json_path: Option<String>,`: `json_view: cli::JsonView,`. Em `DocOpts::from_cli`, depois de `json_path: cli.json_path.clone(),`: `json_view: cli.json_view,`. No braço `cli::Mode::Json` de `render_doc`, trocar a chamada por:

```rust
        cli::Mode::Json => {
            let opts = data::JsonOpts {
                path: doc.json_path.clone(),
                view: doc.json_view,
                head: doc.head,
                cols: doc.cols.clone(),
            };
            data::render_json(text, &opts, width)
        }
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test`
Expected: todos PASS (os 14 testes novos de `data::` incluídos).

- [ ] **Step 6: Teste de integração (falha primeiro)**

Em `tests/cli.rs`, junto dos testes de JSON:

```rust
#[test]
fn json_lista_de_objetos_vira_tabela_por_padrao() {
    let out = mdv(&["json", "-p", "-"], "[{\"id\":1,\"nome\":\"Ana\"},{\"id\":2,\"nome\":\"Bia\"}]");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("│ nome") && text.contains("2 de 2 itens"), "{text}");
}

#[test]
fn json_as_text_forca_a_vista_indentada() {
    let out = mdv(&["json", "-p", "--as", "text", "-"], "[{\"id\":1}]");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("lista · 1 itens") && text.contains("\"id\": 1"), "{text}");
}

#[test]
fn as_com_markdown_e_erro_de_uso() {
    let out = mdv(&["--as", "table", "-"], "# a");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--as"));
}
```

Run: `cargo test --test cli json_ as_com`
Expected: PASS (a implementação já está ligada; se algum falhar, é bug do wiring).

- [ ] **Step 7: Clippy e commit**

Run: `cargo clippy -- -W clippy::pedantic`
Expected: sem warnings.

```bash
git add src/data.rs src/main.rs tests/cli.rs
git commit -m "feat(dados): mdv json em tabela (lista de objetos, JSONL, chave/valor) com --as table"
```

---

### Task 4: Vista de esquema (`--as schema`)

**Files:**
- Modify: `src/data.rs`
- Test: `src/data.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `JsonOpts`, `render_json` (Task 3).
- Produces:
  ```rust
  pub fn schema_text(v: &Value) -> String
  ```
  Formato: primeira linha é o tipo da raiz; filhos indentados em 2 espaços. `lista · N × <tipo>` para listas (`lista · vazia` se vazia), `chave?: tipo` para chave ausente em alguns itens, `tipo | tipo` para tipos mistos, `…` quando um teto foi atingido.

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/data.rs`:

```rust
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
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test data::tests::esquema`
Expected: erro de compilação (`schema_text`, `MAX_SCHEMA_*` não existem).

- [ ] **Step 3: Implementar em `src/data.rs`**

Antes de `pub fn render_json`:

```rust
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

fn render_json_schema(sel: &Value, width: usize) -> Rendered {
    render_code_only(&schema_text(sel), "", width)
}
```

Trocar o braço provisório em `render_json`:

```rust
        JsonView::Schema => render_json_schema(sel, width),
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test data::`
Expected: todos PASS. Se `esquema_para_na_profundidade_maxima` falhar por contagem de linhas, conferir que `depth >= MAX_SCHEMA_DEPTH` retorna antes de gerar filhos.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy -- -W clippy::pedantic`
Expected: sem warnings (se `pedantic` reclamar do tamanho de `group`, extraia o bloco de objetos para `fn object_children(...)` sem mudar o comportamento).

```bash
git add src/data.rs
git commit -m "feat(dados): mdv json --as schema (chaves e tipos, com amostra e tetos)"
```

---

### Task 5: Tecla `v`, rótulo na barra de status e ajuda

**Files:**
- Modify: `src/data.rs`, `src/main.rs`, `tests/cli.rs`
- Test: `src/data.rs`, `src/main.rs` (módulo `tests`), `tests/cli.rs`

**Interfaces:**
- Consumes: `auto_view_of_text` (Task 3), `JsonView` (Task 1).
- Produces:
  ```rust
  pub fn next_view(current: JsonView) -> JsonView   // Auto|Text→Table, Table→Schema, Schema→Text
  ```

- [ ] **Step 1: Escrever os testes que falham**

Em `src/data.rs`, módulo `tests`:

```rust
    #[test]
    fn ciclo_de_vistas() {
        assert_eq!(next_view(JsonView::Auto), JsonView::Table);
        assert_eq!(next_view(JsonView::Text), JsonView::Table);
        assert_eq!(next_view(JsonView::Table), JsonView::Schema);
        assert_eq!(next_view(JsonView::Schema), JsonView::Text);
    }
```

Em `src/main.rs`, módulo `tests` (usa `test_app`; ela sempre cria o `Cli` com `x.md`, então o modo JSON é ajustado direto em `app.doc`):

```rust
    fn tecla(app: &mut App, c: char) {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    #[test]
    fn v_alterna_as_vistas_do_json() {
        let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
        app.doc.mode = cli::Mode::Json;
        tecla(&mut app, 'v'); // Auto já era tabela: o próximo é o esquema
        assert_eq!(app.doc.json_view, cli::JsonView::Schema);
        assert!(app.dirty);
        tecla(&mut app, 'v');
        assert_eq!(app.doc.json_view, cli::JsonView::Text);
        tecla(&mut app, 'v');
        assert_eq!(app.doc.json_view, cli::JsonView::Table);
    }

    #[test]
    fn v_em_json_que_nao_e_lista_comeca_pela_tabela() {
        let mut app = test_app(Source::Stdin, r#"{"a":1}"#);
        app.doc.mode = cli::Mode::Json;
        tecla(&mut app, 'v');
        assert_eq!(app.doc.json_view, cli::JsonView::Table);
    }

    #[test]
    fn v_fora_do_modo_json_nao_faz_nada() {
        let mut app = test_app(Source::Stdin, "# a");
        app.dirty = false;
        tecla(&mut app, 'v');
        assert_eq!(app.doc.json_view, cli::JsonView::Auto);
        assert!(!app.dirty);
    }

    #[test]
    fn barra_mostra_a_vista_do_json_quando_escolhida() {
        let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
        app.doc.mode = cli::Mode::Json;
        tecla(&mut app, 'v');
        assert!(app.status_left().contains("[esquema]"), "{}", app.status_left());
    }
```

Em `tests/cli.rs`, estender `ajuda_documenta_subcomandos_e_opcoes`: acrescentar `"--as"` à lista de itens exigidos.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test ciclo_de_vistas v_ barra_mostra ajuda_documenta`
Expected: erro de compilação (`next_view` não existe) e, depois, falhas de asserção.

- [ ] **Step 3: Implementar**

Em `src/data.rs`, depois de `auto_view_of_text`:

```rust
/// Próxima vista no ciclo da tecla `v`: tabela, esquema, texto.
pub fn next_view(current: JsonView) -> JsonView {
    match current {
        JsonView::Auto | JsonView::Text => JsonView::Table,
        JsonView::Table => JsonView::Schema,
        JsonView::Schema => JsonView::Text,
    }
}
```

Em `src/main.rs`, em `App::on_key`, junto do braço `KeyCode::Char('m')`:

```rust
            KeyCode::Char('v') if self.doc.mode == cli::Mode::Json => {
                let current = if self.doc.json_view == cli::JsonView::Auto {
                    data::auto_view_of_text(&self.md, self.doc.json_path.as_deref())
                } else {
                    self.doc.json_view
                };
                self.doc.json_view = data::next_view(current);
                self.dirty = true;
            }
```

Em `status_left`, trocar a linha que monta `s` por:

```rust
        let view = match (self.doc.mode, self.doc.json_view) {
            (cli::Mode::Json, cli::JsonView::Text) => "  [texto]",
            (cli::Mode::Json, cli::JsonView::Table) => "  [tabela]",
            (cli::Mode::Json, cli::JsonView::Schema) => "  [esquema]",
            _ => "",
        };
        let mut s = format!(
            " {}  {pct}%{}{view}",
            self.name,
            if self.raw { "  [fonte]" } else { "" }
        );
```

No `HELP`, na seção `OPÇÕES DE JSON`, acrescentar abaixo de `--path`:

```
        --as <vista>   auto (padrão), text, table ou schema
```

e na seção `TECLAS`, abaixo de `m`:

```
    v                  (JSON) alterna tabela / esquema / texto
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test && cargo clippy -- -W clippy::pedantic`
Expected: todos PASS; sem warnings.

- [ ] **Step 5: Verificação manual**

Run: `printf '[{"id":1,"nome":"Ana","end":{"cidade":"Recife"}},{"id":2,"nome":"Bia"}]' | cargo run --release -- json -p -` e repita com `--as schema` e `--as text`.
Expected: tabela com colunas `id`, `nome`, `end.cidade` e rodapé `2 de 2 itens · 3 colunas`; depois o esquema; depois o JSON indentado. Na TUI (`cargo run --release -- json samples/itens.json`, depois da Task 6), `v` percorre as vistas e a barra mostra `[esquema]`, `[texto]`, `[tabela]`.

- [ ] **Step 6: Commit**

```bash
git add src/data.rs src/main.rs tests/cli.rs
git commit -m "feat(dados): tecla v alterna tabela, esquema e texto no JSON"
```

---

### Task 6: Amostras e documentação

**Files:**
- Create: `samples/itens.json`, `samples/eventos.jsonl`
- Modify: `README.md`, `CLAUDE.md`, `docs/pendencias.md`

- [ ] **Step 1: Amostras**

`samples/itens.json`:

```json
[
  {"id": 1, "nome": "Ana", "nota": 9.5, "endereco": {"cidade": "Recife", "uf": "PE"}, "tags": ["a", "b"]},
  {"id": 2, "nome": "Bia", "nota": 10, "endereco": {"cidade": "São Paulo", "uf": "SP"}},
  {"id": 3, "nome": "Caio", "nota": 7.25, "ativo": true}
]
```

`samples/eventos.jsonl` (um objeto por linha):

```
{"hora":"10:00:01","evento":"inicio","detalhe":{"agente":"planejador"}}
{"hora":"10:00:07","evento":"ferramenta","detalhe":{"nome":"busca","ms":412}}
{"hora":"10:00:09","evento":"fim","detalhe":{"ok":true}}
```

- [ ] **Step 2: README**

Na seção "JSON, CSV e TSV" de `README.md`, trocar o primeiro bloco de exemplos e o parágrafo seguinte por:

````markdown
```bash
mdv json itens.json                    # lista de objetos (e JSONL) vira tabela
mdv json dados.json --as schema        # só chaves e tipos, sem os valores
mdv json dados.json --as text          # JSON indentado e colorido
mdv json dados.json --path .itens[0]   # só um trecho
mdv csv vendas.csv --head 20 --cols produto,total
mdv tsv planilha.tsv --no-header
mdv csv dados.txt --delimiter ';'
```

Uma lista de objetos (ou JSONL) abre como **tabela**: uma coluna por chave, objetos aninhados achatados como `endereco.cidade` e listas mostradas de forma compacta. `--head` e `--cols` valem também aqui. Os outros JSONs abrem **indentados**, com um resumo de tipo e tamanho. `--as table|schema|text` força a vista, e na TUI a tecla `v` alterna entre tabela, esquema e texto. O **esquema** mostra o tipo de cada campo (`itens: lista · 120 × objeto`, `tags?: ...` quando o campo falta em alguns itens) a partir de uma amostra dos 1000 primeiros itens.

O formato também é deduzido da extensão (`.json`, `.jsonl`, `.csv`, `.tsv`) e funciona com `-` (stdin). Tabelas mostram até 200 colunas (o rodapé avisa `+N omitidas`) e cortam células com mais de 500 caracteres; use `--cols` para tabelas largas, pois não há rolagem horizontal. CSV/TSV têm colunas numéricas alinhadas à direita e rodapé `N de M linhas · K colunas`. A entrada de dados é limitada a 5 MiB.
````

- [ ] **Step 3: CLAUDE.md**

Na linha de `src/data.rs` em "Arquitetura", substituir o texto por: "visualizadores de dados: JSON/JSONL (`render_json` com as vistas texto, tabela e esquema, escolhidas por `--as`/tecla `v`) e CSV/TSV (`render_table_data`); as duas tabelas passam por `finish_table` (tetos `MAX_TABLE_COLS`/`MAX_CELL_CHARS`), reaproveitando `render_code_only` e `render_table_only`." E na lista de comandos, acrescentar `cargo run --release -- -p json samples/itens.json   # tabela; --as schema para o esquema`.

- [ ] **Step 4: Pendências**

Em `docs/pendencias.md`, marcar o primeiro item da seção "Segurança e robustez" ("CSV com milhões de colunas em uma linha") como feito: trocar `- [ ]` por `- [x]` e acrescentar ao fim da linha ` Resolvido em F7.6: tetos \`MAX_TABLE_COLS\` e \`MAX_CELL_CHARS\`.`

- [ ] **Step 5: Verificação final**

Run: `cargo test && cargo clippy -- -W clippy::pedantic && cargo build --release && for f in samples/itens.json samples/eventos.jsonl; do ./target/release/mdv -p $f > /dev/null || echo FALHOU $f; done`
Expected: testes PASS, clippy limpo, nenhum `FALHOU`.

- [ ] **Step 6: Commit**

```bash
git add samples/itens.json samples/eventos.jsonl README.md CLAUDE.md docs/pendencias.md
git commit -m "docs: vistas de tabela e esquema do JSON e amostras"
```

---

## Self-Review

**Cobertura do pedido (F7.6):** tabela (Task 3, também JSONL), esquema (Task 4), escolha por flag (Task 1) e por tecla (Task 5), tetos de colunas/células (Task 2), documentação e amostras (Task 6). A **árvore recolhível** ficou de fora de propósito (decisão registrada no `PLANO.md`: entra junto com o fold da F7.3, pois ambos mexem no `Viewport`).

**Placeholders:** nenhum. A única ponte provisória (braço `Schema` retornando erro na Task 3) é explícita e substituída na Task 4.

**Consistência de tipos:** `JsonView {Auto, Text, Table, Schema}` e `Cli.json_view` (Task 1) usados com os mesmos nomes em `JsonOpts.view`, `DocOpts.json_view`, `next_view` e `auto_view_of_text`. `finish_table(rows, has_header, data_total, omitted_cols, cols, width, unit)` tem a mesma ordem de argumentos nas chamadas de CSV (Task 2) e JSON (Task 3). `render_json(text, &JsonOpts, width)` é a única assinatura a partir da Task 3; os testes antigos são migrados na própria Task 3.

**Riscos conhecidos:** (1) `group` compara `kinds.contains` em vetor pequeno (no máximo 6 tipos), sem custo relevante. (2) A tabela em modo JSON usa só os `head` primeiros itens para decidir as colunas (padrão 1000): chaves que só aparecem depois não viram coluna. É o mesmo corte do `--head` e está documentado no rodapé (`N de M itens`). (3) `docs/superpowers/specs/...fase-7...` e `PLANO.md` são arquivos não versionados neste repositório; a seção F7.6 foi acrescentada neles, mas só este plano e o código entram nos commits.
