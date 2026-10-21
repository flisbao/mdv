//! Tabelas do HTML do Outlook: tabela de dados vira GFM; tabela de layout vira sequência de blocos.

use crate::escape;
use crate::html::{kids, Ctx, Flow};
use crate::MAX_TABLE_COLS;
use scraper::ElementRef;

type Grade<'a> = Vec<Vec<(ElementRef<'a>, usize)>>;

/// Converte uma tabela: GFM se for tabela de dados; senão, cada célula vira blocos em ordem.
pub(crate) fn tabela(c: &Ctx<'_>, el: ElementRef<'_>, depth: usize, cell: bool) -> Result<Vec<String>, String> {
    let mut grade: Grade<'_> = Vec::new();
    for filho in el.child_elements() {
        match filho.value().name() {
            "tr" => grade.push(celulas(filho)),
            "thead" | "tbody" | "tfoot" => {
                for tr in filho.child_elements().filter(|t| t.value().name() == "tr") {
                    grade.push(celulas(tr));
                }
            }
            _ => {}
        }
    }
    if grade.is_empty() {
        return Ok(vec![]);
    }
    if eh_tabela_de_dados(&grade) {
        return tabela_gfm(c, &grade, depth);
    }
    let mut f = Flow::new(c, cell, true);
    for (cel, _) in grade.iter().flatten() {
        c.check()?;
        f.element(*cel, depth + 1)?;
    }
    Ok(f.finish())
}

fn celulas(tr: ElementRef<'_>) -> Vec<(ElementRef<'_>, usize)> {
    tr.child_elements()
        .filter(|c| matches!(c.value().name(), "td" | "th"))
        .map(|c| {
            let span = c.attr("colspan").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1);
            (c, span.clamp(1, MAX_TABLE_COLS))
        })
        .collect()
}

/// Tabela de dados: pelo menos duas linhas com duas colunas (contando `colspan`) e nenhuma
/// célula com outra tabela dentro.
fn eh_tabela_de_dados(grade: &Grade<'_>) -> bool {
    let largas = grade.iter().filter(|r| r.iter().map(|(_, s)| s).sum::<usize>() >= 2).count();
    let aninhada = grade.iter().flatten().any(|(c, _)| {
        c.descendants().any(|n| n.value().as_element().is_some_and(|e| e.name() == "table"))
    });
    largas >= 2 && !aninhada
}

/// GFM: a primeira linha é o cabeçalho; `colspan` repete a célula vazia; colunas até `MAX_TABLE_COLS`.
fn tabela_gfm(c: &Ctx<'_>, grade: &Grade<'_>, depth: usize) -> Result<Vec<String>, String> {
    let colunas = grade
        .iter()
        .map(|r| r.iter().map(|(_, s)| *s).sum::<usize>())
        .max()
        .unwrap_or(0)
        .clamp(1, MAX_TABLE_COLS);
    let mut linhas = Vec::new();
    for (i, linha) in grade.iter().enumerate() {
        c.check()?;
        let mut casas: Vec<String> = Vec::new();
        for (cel, span) in linha {
            if casas.len() >= colunas {
                break;
            }
            let mut f = Flow::new(c, true, true);
            f.nodes(&kids(*cel), 0, depth + 1)?;
            let md = f.finish().join(" ").replace('\n', " ");
            casas.push(escape::cap(&md));
            casas.extend(std::iter::repeat_n(String::new(), span - 1));
        }
        casas.truncate(colunas);
        casas.resize(colunas, String::new());
        let md = format!("| {} |", casas.join(" | "));
        c.gasta(md.len());
        linhas.push(md);
        if i == 0 {
            let sep = format!("| {} |", vec!["---"; colunas].join(" | "));
            c.gasta(sep.len());
            linhas.push(sep);
        }
    }
    Ok(vec![linhas.join("\n")])
}
