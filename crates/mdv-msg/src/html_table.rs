//! Tabelas do HTML do Outlook: tabela de dados vira GFM; tabela de layout vira sequência de blocos.

use crate::escape;
use crate::html::{kids, Ctx, Flow};
use crate::{MAX_CELL_CHARS, MAX_TABLE_COLS};
use scraper::{ElementRef, Node};
use std::collections::HashMap;

/// Elementos que, dentro de uma célula, indicam conteúdo de texto corrido (layout, não dados).
const BLOCOS_CELULA: &[&str] = &[
    "p", "div", "ul", "ol", "li", "blockquote", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "table",
];

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
        let tentativa = c.tentativa();
        if let Some(md) = tabela_gfm(&tentativa, &grade, depth)? {
            c.aceita(&tentativa);
            return Ok(md);
        }
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

/// Tabela de dados: pelo menos duas linhas com duas colunas (contando `colspan`), nenhuma célula com
/// outra tabela dentro, nenhuma com mais de um bloco de texto e nenhuma com texto acima de
/// `MAX_CELL_CHARS`. Tabela de layout (e-mail do Outlook) não é de dados: vira parágrafos, sem perda.
fn eh_tabela_de_dados(grade: &Grade<'_>) -> bool {
    let largas = grade.iter().filter(|r| r.iter().map(|(_, s)| s).sum::<usize>() >= 2).count();
    let aninhada = grade.iter().flatten().any(|(c, _)| {
        c.descendants().any(|n| n.value().as_element().is_some_and(|e| e.name() == "table"))
    });
    let complexa = grade.iter().flatten().any(|(c, _)| varios_blocos(*c) || texto_longo(*c));
    largas >= 2 && !aninhada && !complexa
}

/// Mais de um bloco de saída com texto. Wrappers como `div > p` contam como um bloco; irmãos `p` contam separados.
fn varios_blocos(cel: ElementRef<'_>) -> bool {
    // Reverso da ordem de preordem: filhos são calculados antes do pai sem recursão no stack Rust.
    let mut elementos = vec![cel];
    elementos.extend(cel.descendants().filter_map(ElementRef::wrap));
    let mut info = HashMap::new();
    for el in elementos.into_iter().rev() {
        let mut blocos_filhos = 0usize;
        let mut texto_inline = false;
        for filho in el.children() {
            match filho.value() {
                Node::Text(t) => texto_inline |= !t.text.trim().is_empty(),
                Node::Element(_) => {
                    if let Some(&(blocos, inline)) = info.get(&filho.id()) {
                        blocos_filhos += blocos;
                        texto_inline |= inline;
                    }
                }
                _ => {}
            }
        }
        let bloco = BLOCOS_CELULA.contains(&el.value().name());
        let contagem = if bloco {
            blocos_filhos + usize::from(texto_inline)
        } else {
            blocos_filhos
        };
        let inline = !bloco && texto_inline;
        info.insert(el.id(), (contagem, inline));
    }
    info.get(&cel.id()).is_some_and(|&(n, inline)| n + usize::from(inline && n > 0) > 1)
}

/// Texto da célula acima de `MAX_CELL_CHARS` caracteres (para ao passar do teto).
fn texto_longo(cel: ElementRef<'_>) -> bool {
    let mut n = 0usize;
    for t in cel.text() {
        n += t.chars().count();
        if n > MAX_CELL_CHARS {
            return true;
        }
    }
    false
}

/// GFM: a primeira linha é o cabeçalho; `colspan` repete a célula vazia; colunas até `MAX_TABLE_COLS`.
fn tabela_gfm(c: &Ctx<'_>, grade: &Grade<'_>, depth: usize) -> Result<Option<Vec<String>>, String> {
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
            // O escape pode dobrar o tamanho do texto. Se o Markdown renderizado passa do teto,
            // reclassifica a tabela como layout para não cortar conteúdo da conversa.
            if md.chars().count() > MAX_CELL_CHARS {
                return Ok(None);
            }
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
    Ok(Some(vec![linhas.join("\n")]))
}
