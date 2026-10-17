//! De onde vêm as planilhas: CSV/TSV (lido com `csv`) e tabelas de um Markdown (lidas com
//! `pulldown-cmark`). As duas passam por `build_sheet`, que aplica os tetos do Excel.

use mdv_text::{find_section, headings, parse_options, sanitize, section_range};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::model::{
    cell_from_text, sheet_name, Cell, Sheet, Workbook, MAX_CELL_CHARS, MAX_COLS, MAX_ROWS, MAX_SHEETS,
};
use crate::Budget;

/// Tetos de uma planilha; `EXCEL` são os reais, os testes usam valores pequenos.
pub(crate) struct Limits {
    pub rows: usize,
    pub cols: usize,
    pub cell_chars: usize,
}

impl Limits {
    pub const EXCEL: Limits = Limits { rows: MAX_ROWS, cols: MAX_COLS, cell_chars: MAX_CELL_CHARS };
}

/// Monta uma aba a partir de linhas de texto, cortando (com aviso) o que passar dos tetos.
/// Na linha de cabeçalho tudo é texto, mesmo que pareça número (`2024`).
pub(crate) fn build_sheet(
    name: String,
    header: bool,
    rows: impl Iterator<Item = Result<Vec<String>, String>>,
    limits: &Limits,
    budget: &dyn Budget,
    warnings: &mut Vec<String>,
) -> Result<Sheet, String> {
    let mut sheet = Sheet { name, header, rows: Vec::new() };
    let (mut cut_rows, mut cut_cols, mut cut_cells) = (false, false, false);
    for rec in rows {
        let rec = rec?;
        budget.check()?;
        if sheet.rows.len() >= limits.rows {
            cut_rows = true;
            break;
        }
        cut_cols |= rec.len() > limits.cols;
        let is_header = header && sheet.rows.is_empty();
        let row: Vec<Cell> = rec
            .into_iter()
            .take(limits.cols)
            .map(|raw| {
                let clean = sanitize(&raw);
                let text: String = if clean.chars().count() > limits.cell_chars {
                    cut_cells = true;
                    clean.chars().take(limits.cell_chars).collect()
                } else {
                    clean.into_owned()
                };
                if is_header { Cell::Text(text) } else { cell_from_text(&text) }
            })
            .collect();
        sheet.rows.push(row);
    }
    let aba = sheet.name.clone();
    if cut_rows {
        warnings.push(format!("aba \"{aba}\": só as primeiras {} linhas foram exportadas (limite do Excel)", limits.rows));
    }
    if cut_cols {
        warnings.push(format!("aba \"{aba}\": só as primeiras {} colunas foram exportadas (limite do Excel)", limits.cols));
    }
    if cut_cells {
        warnings.push(format!("aba \"{aba}\": células cortadas em {} caracteres (limite do Excel)", limits.cell_chars));
    }
    Ok(sheet)
}

/// CSV/TSV inteiro → uma aba chamada `name`. `no_header`: a primeira linha é dado.
pub fn from_csv(
    text: &str,
    delimiter: u8,
    no_header: bool,
    name: &str,
    budget: &dyn Budget,
    warnings: &mut Vec<String>,
) -> Result<Workbook, String> {
    budget.check()?;
    let text = text.trim_start_matches('\u{feff}');
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let records = rdr.records().map(|r| match r {
        Ok(rec) => Ok(rec.iter().map(str::to_string).collect::<Vec<String>>()),
        Err(e) => Err(format!("CSV inválido: {e}")),
    });
    let sheet = build_sheet(sheet_name(name, &[]), !no_header, records, &Limits::EXCEL, budget, warnings)?;
    if sheet.rows.is_empty() {
        return Err("arquivo vazio".to_string());
    }
    Ok(Workbook { sheets: vec![sheet] })
}

/// Quais tabelas do Markdown exportar.
pub struct MdSelect<'a> {
    /// Só as tabelas da seção com este título (busca de `mdv-tools section`).
    pub section: Option<&'a str>,
    /// Só a N-ésima tabela do documento (a partir de 1).
    pub table: Option<usize>,
}

struct MdTable {
    /// Byte do fonte onde a tabela começa.
    start: usize,
    /// `rows[0]` é o cabeçalho.
    rows: Vec<Vec<String>>,
}

/// Tabelas do Markdown, em ordem. Tabela dentro de bloco de código não é tabela (o `pulldown-cmark`
/// já não a reconhece). O texto da célula é o texto puro: negrito, código, link e imagem perdem a marcação.
fn md_tables(md: &str, budget: &dyn Budget) -> Result<Vec<MdTable>, String> {
    budget.check_parse(md.len())?;
    let mut out: Vec<MdTable> = Vec::new();
    let mut cur: Option<MdTable> = None;
    let mut row: Vec<String> = Vec::new();
    let mut cell: Option<String> = None;
    for (ev, range) in Parser::new_ext(md, parse_options()).into_offset_iter() {
        budget.check()?;
        match ev {
            Event::Start(Tag::Table(_)) => cur = Some(MdTable { start: range.start, rows: Vec::new() }),
            Event::Start(Tag::TableCell) => cell = Some(String::new()),
            Event::Text(t) | Event::Code(t) => {
                if let Some(c) = &mut cell {
                    c.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(c) = &mut cell {
                    c.push(' ');
                }
            }
            Event::End(TagEnd::TableCell) => row.push(cell.take().unwrap_or_default().trim().to_string()),
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(t) = &mut cur {
                    t.rows.push(std::mem::take(&mut row));
                }
            }
            Event::End(TagEnd::Table) => out.extend(cur.take()),
            _ => {}
        }
    }
    Ok(out)
}

/// Tabelas do Markdown → uma aba por tabela, nomeada pelo título mais próximo acima dela.
pub fn from_markdown(
    md: &str,
    select: &MdSelect,
    budget: &dyn Budget,
    warnings: &mut Vec<String>,
) -> Result<Workbook, String> {
    let heads = headings(md);
    let mut tables = md_tables(md, budget)?;
    if let Some(query) = select.section {
        let i = find_section(&heads, query)?;
        let range = section_range(md, &heads, i);
        tables.retain(|t| range.contains(&t.start));
        if tables.is_empty() {
            return Err(format!("a seção \"{}\" não tem tabelas", heads[i].title));
        }
    }
    if tables.is_empty() {
        return Err("o documento não tem tabelas".to_string());
    }
    // Número de cada tabela no documento inteiro (para o nome padrão) antes de selecionar.
    let mut numbered: Vec<(usize, MdTable)> = tables.into_iter().enumerate().map(|(i, t)| (i + 1, t)).collect();
    if let Some(n) = select.table {
        if n > numbered.len() {
            return Err(format!("--table {n}: o documento tem {} tabela(s)", numbered.len()));
        }
        numbered = vec![numbered.swap_remove(n - 1)];
    }
    if numbered.len() > MAX_SHEETS {
        warnings.push(format!("só as primeiras {MAX_SHEETS} tabelas foram exportadas"));
        numbered.truncate(MAX_SHEETS);
    }
    let mut sheets: Vec<Sheet> = Vec::new();
    for (n, table) in numbered {
        let used: Vec<String> = sheets.iter().map(|s| s.name.clone()).collect();
        let before = heads.partition_point(|h| h.start < table.start);
        let raw = before.checked_sub(1).map_or_else(|| format!("Tabela {n}"), |i| heads[i].title.clone());
        let name = sheet_name(&raw, &used);
        let rows = table.rows.into_iter().map(Ok);
        sheets.push(build_sheet(name, true, rows, &Limits::EXCEL, budget, warnings)?);
    }
    Ok(Workbook { sheets })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::model::Cell;
    use crate::testutil::Exhausted;
    use crate::Unlimited;

    fn t(s: &str) -> Cell {
        Cell::Text(s.to_string())
    }

    fn csv(text: &str) -> Result<Workbook, String> {
        from_csv(text, b',', false, "dados", &Unlimited, &mut Vec::new())
    }

    fn md(text: &str, section: Option<&str>, table: Option<usize>) -> Result<Workbook, String> {
        from_markdown(text, &MdSelect { section, table }, &Unlimited, &mut Vec::new())
    }

    #[test]
    fn csv_simples_vira_texto_e_numero_com_cabecalho() {
        let wb = csv("nome,qtd\nana,3\nrui,4.5\n").unwrap();
        let s = &wb.sheets[0];
        assert_eq!(s.name, "dados");
        assert!(s.header);
        assert_eq!(s.rows[0], vec![t("nome"), t("qtd")]);
        assert_eq!(s.rows[1], vec![t("ana"), Cell::Number(3.0)]);
        assert_eq!(s.rows[2], vec![t("rui"), Cell::Number(4.5)]);
    }

    #[test]
    fn cabecalho_numerico_continua_texto() {
        let wb = csv("2023,2024\n1,2\n").unwrap();
        assert_eq!(wb.sheets[0].rows[0], vec![t("2023"), t("2024")]);
    }

    #[test]
    fn sem_cabecalho_a_primeira_linha_e_dado() {
        let wb = from_csv("1,2\n3,4\n", b',', true, "d", &Unlimited, &mut Vec::new()).unwrap();
        assert!(!wb.sheets[0].header);
        assert_eq!(wb.sheets[0].rows[0], vec![Cell::Number(1.0), Cell::Number(2.0)]);
    }

    #[test]
    fn tsv_usa_tab_e_aspas_do_csv_funcionam() {
        let wb = from_csv("a\tb\n\"x\ty\"\t2\n", b'\t', false, "d", &Unlimited, &mut Vec::new()).unwrap();
        assert_eq!(wb.sheets[0].rows[1], vec![t("x\ty"), Cell::Number(2.0)]);
        let wb = csv("a,b\n\"1,\n2\",\"diz \"\"oi\"\"\"\n").unwrap();
        assert_eq!(wb.sheets[0].rows[1], vec![t("1,\n2"), t("diz \"oi\"")]);
    }

    #[test]
    fn bom_linhas_desiguais_e_ids_com_zero_a_esquerda() {
        let wb = csv("\u{feff}id,nome\n007,ana\n8\n").unwrap();
        let r = &wb.sheets[0].rows;
        assert_eq!(r[0][0], t("id"), "BOM removido");
        assert_eq!(r[1][0], t("007"));
        assert_eq!(r[2], vec![Cell::Number(8.0)], "linha curta fica curta; o escritor completa");
    }

    #[test]
    fn formulas_e_controles_ficam_como_texto_limpo() {
        let wb = csv("h\n=1+1\n\"a\u{7}b\"\n").unwrap();
        assert_eq!(wb.sheets[0].rows[1], vec![t("=1+1")]);
        assert_eq!(wb.sheets[0].rows[2], vec![t("a\u{FFFD}b")]);
    }

    #[test]
    fn csv_vazio_e_erro_e_so_cabecalho_e_planilha_valida() {
        assert!(csv("").unwrap_err().contains("vazio"));
        assert!(csv("\u{feff}").unwrap_err().contains("vazio"));
        assert_eq!(csv("a,b\n").unwrap().sheets[0].rows.len(), 1);
    }

    #[test]
    fn csv_malformado_nao_entra_em_panico() {
        let r = from_csv("a,b\n\"aberta,1\n", b',', false, "d", &Unlimited, &mut Vec::new());
        // O crate `csv` aceita aspas não fechadas até o fim; o que importa é não haver pânico.
        assert!(r.is_ok() || r.unwrap_err().contains("CSV inválido"));
    }

    #[test]
    fn budget_esgotado_aborta_csv_e_markdown() {
        let e = from_csv("a\n1\n", b',', false, "d", &Exhausted, &mut Vec::new()).unwrap_err();
        assert!(e.contains("memória"), "{e}");
        let e = from_markdown("| a |\n|---|\n| 1 |\n", &MdSelect { section: None, table: None }, &Exhausted, &mut Vec::new())
            .unwrap_err();
        assert!(e.contains("memória"), "{e}");
    }

    // ---- tetos (valores reduzidos: nunca se gera entrada gigante) ----

    fn linhas(rows: &[&[&str]]) -> std::vec::IntoIter<Result<Vec<String>, String>> {
        rows.iter().map(|r| Ok(r.iter().map(|s| s.to_string()).collect())).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn teto_de_linhas_corta_com_aviso() {
        let lim = Limits { rows: 2, cols: 10, cell_chars: 100 };
        let mut w = Vec::new();
        let s = build_sheet("d".into(), true, linhas(&[&["h"], &["1"], &["2"], &["3"]]), &lim, &Unlimited, &mut w).unwrap();
        assert_eq!(s.rows.len(), 2);
        assert!(w.iter().any(|x| x.contains("linhas")), "{w:?}");
    }

    #[test]
    fn teto_de_colunas_e_de_caracteres_cortam_com_aviso() {
        let lim = Limits { rows: 10, cols: 2, cell_chars: 3 };
        let mut w = Vec::new();
        let s = build_sheet("d".into(), false, linhas(&[&["abcdef", "b", "c"]]), &lim, &Unlimited, &mut w).unwrap();
        assert_eq!(s.rows[0], vec![t("abc"), t("b")]);
        assert!(w.iter().any(|x| x.contains("colunas")) && w.iter().any(|x| x.contains("caracteres")), "{w:?}");
    }

    #[test]
    fn corte_de_caracteres_respeita_limites_de_char() {
        let lim = Limits { rows: 10, cols: 10, cell_chars: 2 };
        let s = build_sheet("d".into(), false, linhas(&[&["ação"]]), &lim, &Unlimited, &mut Vec::new()).unwrap();
        assert_eq!(s.rows[0], vec![t("aç")]);
    }

    // ---- Markdown ----

    const DOC: &str = "\
# Relatório

intro

## Vendas

| produto | qtd |
|---|--:|
| **maçã** | 3 |
| `pera` \\| uva | [link](http://x.org) |

## Custos

| item | valor |
|---|---|
| luz | 10.5 |

```
| nao | tabela |
|---|---|
| a | b |
```

## Vendas

| mes | total |
|---|---|
| jan | 1 |
";

    #[test]
    fn todas_as_tabelas_viram_abas_nomeadas_pelo_titulo() {
        let wb = md(DOC, None, None).unwrap();
        let nomes: Vec<&str> = wb.sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(nomes, ["Vendas", "Custos", "Vendas (2)"], "bloco de código não é tabela");
        assert!(wb.sheets.iter().all(|s| s.header));
    }

    #[test]
    fn formatacao_dentro_da_celula_vira_texto_puro() {
        let wb = md(DOC, None, None).unwrap();
        let r = &wb.sheets[0].rows;
        assert_eq!(r[0], vec![t("produto"), t("qtd")]);
        assert_eq!(r[1], vec![t("maçã"), Cell::Number(3.0)]);
        assert_eq!(r[2], vec![t("pera | uva"), t("link")]);
    }

    #[test]
    fn section_filtra_as_tabelas_da_secao() {
        let wb = md(DOC, Some("custos"), None).unwrap();
        assert_eq!(wb.sheets.len(), 1);
        assert_eq!(wb.sheets[0].name, "Custos");
        assert_eq!(wb.sheets[0].rows[1], vec![t("luz"), Cell::Number(10.5)]);
    }

    #[test]
    fn section_inexistente_sem_tabela_ou_repetida() {
        assert!(md(DOC, Some("zzz"), None).unwrap_err().contains("zzz"));
        assert!(md("# A\n\ntexto\n", Some("A"), None).unwrap_err().contains("tabela"));
        let wb = md(DOC, Some("Vendas"), None).unwrap();
        assert_eq!(wb.sheets.len(), 1, "igualdade exata escolhe a primeira seção de mesmo título");
    }

    #[test]
    fn table_escolhe_a_enesima_do_documento() {
        let wb = md(DOC, None, Some(2)).unwrap();
        assert_eq!(wb.sheets.len(), 1);
        assert_eq!(wb.sheets[0].name, "Custos");
        let e = md(DOC, None, Some(9)).unwrap_err();
        assert!(e.contains("3 tabela"), "{e}");
    }

    #[test]
    fn documento_sem_tabela_e_erro_claro() {
        assert!(md("# So texto\n", None, None).unwrap_err().contains("não tem tabelas"));
        assert!(md("", None, None).is_err());
    }

    #[test]
    fn tabela_antes_de_qualquer_titulo_vira_tabela_n() {
        let wb = md("| a |\n|---|\n| 1 |\n", None, None).unwrap();
        assert_eq!(wb.sheets[0].name, "Tabela 1");
    }

    #[test]
    fn teto_de_abas() {
        let mut doc = String::new();
        for i in 0..(MAX_SHEETS + 5) {
            doc.push_str(&format!("| h |\n|---|\n| {i} |\n\n"));
        }
        let mut w = Vec::new();
        let wb = from_markdown(&doc, &MdSelect { section: None, table: None }, &Unlimited, &mut w).unwrap();
        assert_eq!(wb.sheets.len(), MAX_SHEETS);
        assert!(w.iter().any(|x| x.contains("tabelas")), "{w:?}");
    }
}
