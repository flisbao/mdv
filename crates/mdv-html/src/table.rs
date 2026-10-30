//! Tabelas: linhas (`tr`) de células (`td`/`th`) com `colspan`, larguras naturais reduzidas até
//! caber, grade (`│` e `─┼─`) para tabelas com borda ou cabeçalho e colunas separadas por dois
//! espaços para tabelas de layout. Cada célula é renderizada por `flow::conteudo`.
//!
//! Custo: uma célula sem tabela aninhada é renderizada duas vezes (medida e definitiva). Uma
//! célula que contém tabela não é medida: ocupa a largura disponível. Assim tabelas aninhadas
//! são renderizadas uma vez por nível, e não 2^n vezes.

use crate::ctx::Ctx;
use crate::flow::{ajusta, conteudo, desloca, medida};
use crate::inline;
use crate::length::Len;
use crate::model::{Face, Line, Rgb};
use crate::style::{compute, Align, Border, Style};
use crate::style_apply::CINZA;
use crate::text::blank;
use scraper::ElementRef;
use unicode_width::UnicodeWidthChar;

/// Menor largura, em colunas, de uma coluna ao reduzir a tabela.
const COLUNA_MIN: usize = 4;

/// Uma célula da tabela: elemento, estilo calculado e o que a tabela precisa saber dela.
struct Celula<'a> {
    el: ElementRef<'a>,
    cs: Style,
    /// Colunas ocupadas (`colspan`, já limitado).
    span: usize,
    /// Largura pedida por `width` (só células de uma coluna), em colunas.
    pref: Option<usize>,
    th: bool,
    /// Contém tabela: a largura natural é a disponível (não se mede a subárvore).
    aninhada: bool,
}

/// Linhas da tabela `el` (estilo `st`) na largura `width`, sem a caixa da própria tabela.
/// `bg` é o fundo atrás da tabela; `depth` a profundidade de blocos.
pub fn table_lines(
    el: ElementRef<'_>,
    st: &Style,
    width: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let (linhas, grade) = coleta(el, st, width, cx)?;
    let nc = linhas
        .iter()
        .map(|l| l.iter().map(|c| c.span).sum::<usize>())
        .max()
        .unwrap_or(0);
    if nc == 0 {
        return Ok(Vec::new());
    }
    let sep = if grade { 3 } else { 2 };

    // Larguras: a maior linha de cada coluna, a preferência de `width` quando há, e mescladas.
    let mut nat = vec![1usize; nc];
    let mut pref: Vec<Option<usize>> = vec![None; nc];
    let mut mesclados: Vec<(usize, usize, usize)> = Vec::new();
    for linha in &linhas {
        let mut col = 0usize;
        for c in linha {
            let m = mede(c, width, bg, depth, cx)?;
            if c.span == 1 {
                if let (Some(n), Some(p)) = (nat.get_mut(col), pref.get_mut(col)) {
                    *n = (*n).max(m);
                    *p = (*p).max(c.pref);
                }
            } else {
                mesclados.push((col, c.span, m));
            }
            col += c.span;
        }
    }
    let mut colw: Vec<usize> = nat
        .iter()
        .zip(pref.iter())
        .map(|(&n, p)| p.unwrap_or(n).min(width).max(1))
        .collect();
    for (inicio, span, m) in mesclados {
        let tem = largura_span(&colw, inicio, span, sep);
        if m > tem {
            if let Some(x) = colw.get_mut(inicio + span - 1) {
                *x += m - tem;
            }
        }
    }
    if largura_span(&colw, 0, nc, sep) > width {
        colw = encolhe(&colw, width, sep);
    }

    let mut out: Vec<Line> = Vec::new();
    for (ri, linha) in linhas.iter().enumerate() {
        // Cada item: linhas da célula, largura e fundo. Células são alinhadas no topo.
        let mut partes: Vec<(Vec<Line>, usize, Option<Rgb>)> = Vec::new();
        let mut col = 0usize;
        for c in linha {
            let w = largura_span(&colw, col, c.span, sep);
            let cbg = inline::fundo_efetivo(&c.cs, bg);
            let h0 = cx.headings.borrow().len();
            let ls = conteudo_celula(c, w, cbg, depth, cx)?;
            desloca(cx, h0, out.len());
            partes.push((ls, w, cbg));
            col += c.span;
        }
        if col < nc {
            let w = largura_span(&colw, col, nc - col, sep);
            partes.push((Vec::new(), w, bg));
        }
        let altura = partes.iter().map(|p| p.0.len()).max().unwrap_or(0).max(1);
        cx.linhas(altura)?;
        for k in 0..altura {
            let mut l: Line = Vec::new();
            for (i, (ls, w, cbg)) in partes.iter().enumerate() {
                if i > 0 {
                    l.extend(separador(grade, bg));
                }
                let celula = ls.get(k).cloned().unwrap_or_else(|| blank(*w, *cbg));
                l.extend(ajusta(celula, *w, *cbg));
            }
            out.push(ajusta(l, width, bg));
        }
        if grade && ri == 0 && linha.iter().any(|c| c.th) {
            let larguras: Vec<usize> = partes.iter().map(|p| p.1).collect();
            out.push(traco(&larguras, width, bg));
        }
    }
    Ok(out)
}

/// Linhas `tr` de `el`: filhos diretos e os de `thead`, `tbody` e `tfoot`. Não entra em
/// tabelas aninhadas.
fn linhas_de(el: ElementRef<'_>) -> Vec<ElementRef<'_>> {
    let mut v = Vec::new();
    for filho in el.children().filter_map(ElementRef::wrap) {
        match filho.value().name() {
            "tr" => v.push(filho),
            "thead" | "tbody" | "tfoot" => v.extend(
                filho
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| e.value().name() == "tr"),
            ),
            _ => {}
        }
    }
    v
}

/// Coleta as linhas de células (até `max_table_cols` colunas) e diz se a tabela tem grade:
/// borda em `table`, `td` ou `th`, ou célula de cabeçalho.
fn coleta<'a>(
    el: ElementRef<'a>,
    st: &Style,
    largura: usize,
    cx: &Ctx<'_>,
) -> Result<(Vec<Vec<Celula<'a>>>, bool), String> {
    let teto = cx.cascade.lim.max_table_cols.max(1);
    let mut linhas = Vec::new();
    let mut grade = st.border != Border::default();
    for tr in linhas_de(el) {
        cx.tick()?;
        let ts = compute(tr, st, cx.cascade);
        if inline::oculto(&ts) {
            continue;
        }
        let mut linha = Vec::new();
        let mut col = 0usize;
        for td in tr.children().filter_map(ElementRef::wrap) {
            let nome = td.value().name();
            if nome != "td" && nome != "th" {
                continue;
            }
            if col >= teto {
                break;
            }
            cx.tick()?;
            let cs = compute(td, &ts, cx.cascade);
            if inline::oculto(&cs) {
                continue;
            }
            let span = colspan(td, teto).min(teto - col);
            let th = nome == "th";
            grade |= th || cs.border != Border::default();
            let pref = if span == 1 { preferida(&cs, largura) } else { None };
            linha.push(Celula {
                el: td,
                cs,
                span,
                pref,
                th,
                aninhada: contem_tabela(td),
            });
            col += span;
        }
        if !linha.is_empty() {
            linhas.push(linha);
        }
    }
    Ok((linhas, grade))
}

/// Valor de `colspan`, entre 1 e `teto`.
fn colspan(td: ElementRef<'_>, teto: usize) -> usize {
    td.value()
        .attr("colspan")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, teto)
}

/// Largura pedida pela célula (`width` em px ou %), se houver.
fn preferida(cs: &Style, largura: usize) -> Option<usize> {
    let l = cs.width?;
    match l {
        Len::Auto => None,
        _ => Some(medida(l, largura)).filter(|&w| w > 0),
    }
}

/// Verdadeiro se a célula contém outra tabela em qualquer nível.
fn contem_tabela(td: ElementRef<'_>) -> bool {
    td.descendants()
        .filter_map(ElementRef::wrap)
        .any(|e| e.value().name() == "table")
}

/// Largura natural de uma célula: a maior linha do conteúdo na largura `largura`, com o
/// alinhamento à esquerda (o recuo do alinhamento não conta). Célula com tabela: `largura`.
fn mede(
    c: &Celula<'_>,
    largura: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<usize, String> {
    if c.aninhada {
        return Ok(largura);
    }
    let h0 = cx.headings.borrow().len();
    let mut cs = c.cs.clone();
    cs.align = Align::Left;
    let cbg = inline::fundo_efetivo(&c.cs, bg);
    let linhas = conteudo(c.el, &cs, largura, cbg, depth + 1, cx)?;
    let linhas = corta_texto(linhas, cx.cascade.lim.max_cell_chars);
    cx.headings.borrow_mut().truncate(h0);
    Ok(largura_natural(&linhas))
}

/// Linhas definitivas da célula na largura `largura`. `th` é centralizado se não tiver alinhamento.
/// O teto `max_cell_chars` vale só para célula folha: célula que contém tabela aninhada nunca é
/// cortada (cortar esconderia o conteúdo das tabelas internas). Os tetos de elementos, linhas e
/// profundidade continuam valendo.
fn conteudo_celula(
    c: &Celula<'_>,
    largura: usize,
    cbg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let mut cs = c.cs.clone();
    if c.th && cs.align == Align::Left {
        cs.align = Align::Center;
    }
    let mut linhas = conteudo(c.el, &cs, largura, cbg, depth + 1, cx)?;
    if !c.aninhada {
        linhas = corta_texto(linhas, cx.cascade.lim.max_cell_chars);
    }
    Ok(linhas.into_iter().map(|l| ajusta(l, largura, cbg)).collect())
}

/// Corta o texto da célula depois de `teto` caracteres que não são espaço, com `…`.
fn corta_texto(linhas: Vec<Line>, teto: usize) -> Vec<Line> {
    let mut usados = 0usize;
    let mut out: Vec<Line> = Vec::new();
    for linha in linhas {
        let mut nova: Line = Vec::new();
        for c in linha {
            if c.ch != ' ' {
                if usados >= teto {
                    let reticencias = inline::celula('…', c.face);
                    if nova.is_empty() {
                        if let Some(anterior) = out.last_mut() {
                            anterior.push(reticencias);
                            return out;
                        }
                    }
                    nova.push(reticencias);
                    out.push(nova);
                    return out;
                }
                usados += 1;
            }
            nova.push(c);
        }
        out.push(nova);
    }
    out
}

/// Largura da maior linha até o último caractere que não é espaço. Mínimo 1.
fn largura_natural(linhas: &[Line]) -> usize {
    linhas
        .iter()
        .map(|l| {
            let mut acumulada = 0usize;
            let mut fim = 0usize;
            for c in l {
                acumulada += UnicodeWidthChar::width(c.ch).unwrap_or(0);
                if c.ch != ' ' {
                    fim = acumulada;
                }
            }
            fim
        })
        .max()
        .unwrap_or(0)
        .max(1)
}

/// Largura ocupada por `span` colunas a partir de `inicio`, com os separadores entre elas.
fn largura_span(colunas: &[usize], inicio: usize, span: usize, sep: usize) -> usize {
    colunas.iter().skip(inicio).take(span).sum::<usize>() + sep * span.saturating_sub(1)
}

/// Reduz as colunas maiores até a soma (com separadores) caber em `largura`: o maior teto `L`
/// (no mínimo `COLUNA_MIN`) com `Σ min(c, L)` cabendo. Se nem o mínimo cabe, todas vão a
/// `COLUNA_MIN` e a linha é cortada depois.
fn encolhe(colunas: &[usize], largura: usize, sep: usize) -> Vec<usize> {
    let alvo = largura.saturating_sub(sep * colunas.len().saturating_sub(1));
    let soma = |teto: usize| colunas.iter().map(|&c| c.min(teto)).sum::<usize>();
    let maior = colunas.iter().copied().max().unwrap_or(0);
    let mut lo = COLUNA_MIN;
    if soma(lo) > alvo {
        return colunas.iter().map(|&c| c.min(lo)).collect();
    }
    let mut hi = maior.max(lo);
    while lo < hi {
        let meio = lo + (hi - lo).div_ceil(2);
        if soma(meio) <= alvo {
            lo = meio;
        } else {
            hi = meio - 1;
        }
    }
    colunas.iter().map(|&c| c.min(lo)).collect()
}

/// Separador entre colunas: ` │ ` na grade, dois espaços na tabela de layout.
fn separador(grade: bool, bg: Option<Rgb>) -> Line {
    if !grade {
        return blank(2, bg);
    }
    let fundo = Face {
        bg,
        ..Face::default()
    };
    let traco = Face {
        fg: Some(CINZA),
        bg,
        ..Face::default()
    };
    vec![
        inline::celula(' ', fundo),
        inline::celula('│', traco),
        inline::celula(' ', fundo),
    ]
}

/// Régua `─┼─` depois do cabeçalho, com as larguras das células da linha.
fn traco(larguras: &[usize], largura: usize, bg: Option<Rgb>) -> Line {
    let f = Face {
        fg: Some(CINZA),
        bg,
        ..Face::default()
    };
    let mut l: Line = Vec::new();
    for (i, &w) in larguras.iter().enumerate() {
        if i > 0 {
            l.push(inline::celula('─', f));
            l.push(inline::celula('┼', f));
            l.push(inline::celula('─', f));
        }
        l.extend((0..w).map(|_| inline::celula('─', f)));
    }
    ajusta(l, largura, bg)
}

#[cfg(test)]
mod tests {
    use crate::guard::Limits;
    use crate::model::{line_width, Rgb};
    use crate::testutil::{render_body, rodar_com, texto};
    use crate::Unlimited;

    fn sem_limite() -> Limits {
        Limits::default()
    }

    #[test]
    fn tabela_simples_com_cabecalho() {
        let l = render_body(
            "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>",
            "",
            20,
        );
        assert_eq!(l[0], "A │ B");
        assert_eq!(l[1], "──┼──");
        assert_eq!(l[2], "1 │ 2");
    }

    #[test]
    fn colspan() {
        let l = render_body(
            "<table><tr><td colspan=\"2\">ab</td></tr><tr><td>1</td><td>2</td></tr></table>",
            "",
            20,
        );
        assert_eq!(l[0], "ab");
        assert_eq!(l[1], "1  2");
    }

    #[test]
    fn celulas_multilinha() {
        let l = render_body(
            "<table><tr><td><p>a</p><p>b</p></td><td>x</td></tr></table>",
            "",
            20,
        );
        assert_eq!(l[0], "a  x");
        assert_eq!(l[2], "b");
    }

    #[test]
    fn tabela_de_layout_sem_grade() {
        let l = render_body("<table><tr><td>a</td><td>b</td></tr></table>", "", 20);
        assert_eq!(l[0], "a  b");
        assert!(l.iter().all(|x| !x.contains('│')), "{l:?}");
    }

    #[test]
    fn tabela_com_borda_do_atributo_desenha_grade() {
        let saida = rodar_com(
            "<table border=\"1\"><tr><td>a</td><td>b</td></tr></table>",
            "",
            20,
            &sem_limite(),
            &Unlimited,
        )
        .unwrap();
        let l: Vec<String> = saida.linhas.iter().map(|x| texto(x)).collect();
        assert_eq!(l[0], "a │ b", "{l:?}");
        assert!(!l[0].contains('▌') && !l[0].contains('▐'), "{l:?}");
    }

    #[test]
    fn cor_de_celula() {
        let saida = rodar_com(
            "<table><tr><td bgcolor=\"#f00\">a</td><td>b</td></tr>\
             <tr><td>muito texto</td><td>c</td></tr></table>",
            "",
            40,
            &sem_limite(),
            &Unlimited,
        )
        .unwrap();
        let vermelho = Some(Rgb(255, 0, 0));
        assert!(saida.linhas[0][..11].iter().all(|c| c.face.bg == vermelho));
    }

    #[test]
    fn alinhamento() {
        let l = render_body(
            "<table><tr><td align=\"right\">a</td></tr><tr><td>bbbbb</td></tr></table>",
            "",
            20,
        );
        assert_eq!(l[0], "    a");
    }

    #[test]
    fn cabecalho_centralizado() {
        let l = render_body(
            "<table><tr><th>ab</th></tr><tr><td>abcdef</td></tr></table>",
            "",
            20,
        );
        assert_eq!(l[0], "  ab");
        assert_eq!(l[2], "abcdef");
    }

    #[test]
    fn tabela_aninhada() {
        let l = render_body(
            "<table><tr><td><table><tr><td>in</td></tr></table></td><td>out</td></tr></table>",
            "",
            30,
        );
        assert!(l[0].contains("in") && l[0].contains("out"), "{l:?}");
        assert!(l[0].find("out") > l[0].find("in"), "{l:?}");
    }

    #[test]
    fn tabela_larga_nao_passa_da_largura() {
        let saida = rodar_com(
            "<table><tr><td>aaaaaaaaaa</td><td>bbbbbbbbbb</td><td>cccccccccc</td></tr></table>",
            "",
            12,
            &sem_limite(),
            &Unlimited,
        )
        .unwrap();
        assert!(saida.linhas.iter().all(|l| line_width(l) == 12));
        assert!(texto(&saida.linhas[0]).contains('…'), "{:?}", saida.linhas[0]);
    }

    #[test]
    fn celula_enorme_e_truncada() {
        let longo = "x".repeat(1000);
        let lim = Limits {
            max_cell_chars: 10,
            ..Default::default()
        };
        let saida = rodar_com(
            &format!("<table><tr><td>{longo}</td></tr></table>"),
            "",
            30,
            &lim,
            &Unlimited,
        )
        .unwrap();
        let xs: usize = saida
            .linhas
            .iter()
            .map(|l| l.iter().filter(|c| c.ch == 'x').count())
            .sum();
        assert!(xs <= 10, "xs = {xs}");
        assert!(saida.linhas.iter().any(|l| l.iter().any(|c| c.ch == '…')));
    }

    #[test]
    fn celula_com_tabelas_aninhadas_nao_e_truncada() {
        // Cada tabela interna tem ~210 caracteres sem espaço; juntas passam de `max_cell_chars`.
        let enchimento = "palavra ".repeat(26);
        let html = format!(
            "<table><tr><td>\
             <table><tr><td>MARCA-UM {enchimento}</td></tr></table>\
             <table><tr><td>MARCA-DOIS {enchimento}</td></tr></table>\
             <table><tr><td>MARCA-TRES {enchimento}</td></tr></table>\
             </td></tr></table>"
        );
        let l = render_body(&html, "", 80);
        let todo = l.join("\n");
        for marca in ["MARCA-UM", "MARCA-DOIS", "MARCA-TRES"] {
            assert!(todo.contains(marca), "falta {marca}: {l:?}");
        }
        assert!(!todo.contains('…'), "célula com tabela não pode ser cortada: {l:?}");
    }

    #[test]
    fn colunas_demais() {
        let lim = Limits {
            max_table_cols: 3,
            ..Default::default()
        };
        let celulas: String = (0..10).map(|i| format!("<td>{i}</td>")).collect();
        let saida = rodar_com(
            &format!("<table><tr>{celulas}</tr></table>"),
            "",
            40,
            &lim,
            &Unlimited,
        )
        .unwrap();
        assert_eq!(texto(&saida.linhas[0]), "0  1  2");
    }

    #[test]
    fn tabela_sem_linhas_nao_panica() {
        for html in ["<table></table>", "<table><tr></tr></table>"] {
            let l = render_body(html, "", 20);
            assert!(l.iter().all(|x| x.trim().is_empty()), "{l:?}");
        }
    }

    #[test]
    fn aninhamento_200_niveis_termina() {
        let html = format!(
            "{}fundo{}",
            "<table><tr><td>".repeat(200),
            "</td></tr></table>".repeat(200)
        );
        let lim = Limits {
            max_walk_depth: 20,
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let r = rodar_com(&html, "", 80, &lim, &Unlimited);
        assert!(t.elapsed().as_secs() < 5, "demorou {:?}", t.elapsed());
        let saida = r.unwrap();
        assert!(saida.linhas.iter().any(|l| texto(l).contains("fundo")));

        let t = std::time::Instant::now();
        let r = rodar_com(&html, "", 80, &sem_limite(), &Unlimited);
        assert!(t.elapsed().as_secs() < 5, "demorou {:?}", t.elapsed());
        match r {
            Ok(s) => assert!(s.linhas.iter().any(|l| texto(l).contains("fundo"))),
            Err(e) => assert!(e.contains("complexa demais"), "{e}"),
        }
    }
}
