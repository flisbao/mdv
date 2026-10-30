//! Flex (linha e coluna) e grid: itens lado a lado, justificação, alinhamento e colunas iguais.
//! Cada item passa por `flow::layout_min`, então pinta o próprio fundo e se centraliza como bloco.
//! A aritmética das larguras do flex de linha (encolher, crescer) fica em `flex_size.rs`.

use crate::ctx::Ctx;
use crate::flex_size::{self, Dimensao};
use crate::flow::{ajusta, desloca, layout_min, medida, pad};
use crate::inline::{self, Item};
use crate::length::{px_to_cols, Len};
use crate::model::{Cell, Line, Rgb};
use crate::style::{compute, Align, AlignItems, Justify, Style, WhiteSpace};
use crate::text::{align, blank, push_text, wrap};
use scraper::ElementRef;

/// Gap, em px, a partir do qual flex coluna e grid separam itens com uma linha em branco.
const GAP_LINHA_PX: f32 = 16.0;
/// Menor largura, em colunas, de uma coluna de grid antes de reduzir o número de colunas.
const COLUNA_MIN: usize = 12;
/// Altura de uma linha de texto, em px (a mesma conversão de `length::px_to_rows`).
const PX_POR_LINHA: f32 = 16.0;
/// Níveis de flex aninhados que medem os itens antes de posicioná-los. Mais fundo que isso, o
/// flex empilha: medir cada nível repete o trabalho do nível de dentro (custo exponencial).
const NIVEL_MEDIDO: usize = 3;
/// Menor largura, em colunas, de um item de flex de linha que encolhe (quando cabe).
const MINIMO_ITEM: usize = 3;

/// Filho de um contêiner flex ou grid: elemento visível ou texto solto com conteúdo.
enum Parte<'a> {
    Elem(ElementRef<'a>, Style),
    Texto(String),
}

/// Junta colunas lado a lado. Colunas mais curtas são completadas com linhas em branco
/// (com `bg`); `gap` células de espaço com `bg` separam uma coluna da outra.
pub(crate) fn side_by_side(
    columns: Vec<Vec<Line>>,
    widths: &[usize],
    gap: usize,
    bg: Option<Rgb>,
) -> Vec<Line> {
    let altura = columns.iter().map(Vec::len).max().unwrap_or(0);
    let mut linhas = Vec::with_capacity(altura);
    for k in 0..altura {
        let mut linha: Line = Vec::new();
        for (i, coluna) in columns.iter().enumerate() {
            if i > 0 {
                linha.extend(blank(gap, bg));
            }
            let w = widths.get(i).copied().unwrap_or(0);
            match coluna.get(k) {
                Some(l) => linha.extend(ajusta(l.clone(), w, bg)),
                None => linha.extend(blank(w, bg)),
            }
        }
        linhas.push(linha);
    }
    linhas
}

/// Layout de `display: flex`: em linha (itens lado a lado, ou empilhados se não cabem) ou em
/// coluna (`flex-direction: column`). `el` é o contêiner, com estilo `st`, na largura `w`.
/// `alvo` é a altura mínima do conteúdo, em linhas (a altura definida do contêiner, ou a da linha
/// de um flex ou grid que o estica); é a referência para `height` em % dos itens.
pub fn flex_lines(
    el: ElementRef<'_>,
    st: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
    alvo: usize,
) -> Result<Vec<Line>, String> {
    let mut ps = partes(el, st, cx)?;
    if ps.is_empty() {
        return Ok(Vec::new());
    }
    if alvo > 0 {
        for p in ps.iter_mut() {
            altura_relativa(p, alvo);
        }
    }
    let nivel = cx.flex_nivel.get();
    cx.flex_nivel.set(nivel.saturating_add(1));
    let medido = nivel < NIVEL_MEDIDO;
    let r = if st.flex_column {
        flex_coluna(&ps, st, w, bg, depth, cx, alvo, medido)
    } else if medido {
        flex_linha(&ps, st, w, bg, depth, cx, alvo)
    } else {
        empilha(&ps, st, w, bg, depth, cx)
    };
    cx.flex_nivel.set(nivel);
    r
}

/// Layout de `display: grid` com `grid-template-columns: repeat(N, …)`: colunas iguais, linha a
/// linha. Com menos de duas colunas de no mínimo `COLUNA_MIN`, empilha como bloco.
pub fn grid_lines(
    el: ElementRef<'_>,
    st: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let ps: Vec<Parte<'_>> = partes(el, st, cx)?
        .into_iter()
        .filter(|p| matches!(p, Parte::Elem(..)))
        .collect();
    let mut n = st.grid_cols.min(cx.cascade.lim.max_grid_cols);
    let g = px_to_cols(st.gap);
    while n > 1 && w.saturating_sub(g * (n - 1)) / n < COLUNA_MIN {
        n -= 1;
    }
    if n < 2 {
        return empilha(&ps, st, w, bg, depth, cx);
    }
    let col_w = w.saturating_sub(g * (n - 1)) / n;
    let mut out: Vec<Line> = Vec::new();
    for linha in ps.chunks(n) {
        let inicio = out.len();
        let h_ini = cx.headings.borrow().len();
        let larguras = vec![col_w; linha.len()];
        let colunas = linha_de_itens(linha, &larguras, st, 0, bg, depth, cx)?;
        let row = side_by_side(colunas, &larguras, g, bg);
        desloca(cx, h_ini, inicio);
        out.extend(row.into_iter().map(|l| ajusta(l, w, bg)));
    }
    Ok(out)
}

/// Filhos visíveis de `el` (elementos ocultos e `<br>` saem; texto só de espaços também).
fn partes<'a>(el: ElementRef<'a>, st: &Style, cx: &Ctx<'_>) -> Result<Vec<Parte<'a>>, String> {
    let mut out = Vec::new();
    for item in inline::filhos(el) {
        match item {
            Item::Texto(t) => {
                if !t.trim().is_empty() {
                    out.push(Parte::Texto(t));
                }
            }
            Item::Elem(c) => {
                cx.tick()?;
                let cs = compute(c, st, cx.cascade);
                if inline::oculto(&cs) || c.value().name() == "br" {
                    continue;
                }
                out.push(Parte::Elem(c, cs));
            }
        }
    }
    Ok(out)
}

/// Altura em porcentagem de um item de flex vira px: a porcentagem do `alvo` (a altura do
/// contêiner, que é definida). Assim `height:100%` e `height:62%` de barras ocupam as linhas certas.
fn altura_relativa(p: &mut Parte<'_>, alvo: usize) {
    if let Parte::Elem(_, cs) = p {
        if let Some(Len::Percent(pc)) = cs.height {
            let linhas = (alvo as f32 * pc / 100.0).round().max(0.0);
            cs.height = Some(Len::Px(linhas * PX_POR_LINHA));
        }
    }
}

/// Renderiza um item na largura `w`. Elementos passam por `layout`; texto solto é quebrado.
fn renderiza(
    p: &Parte<'_>,
    pai: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    renderiza_min(p, pai, w, bg, depth, cx, 0)
}

/// Como `renderiza`, mas um elemento tem pelo menos `min_rows` linhas (ver `flow::layout_min`).
fn renderiza_min(
    p: &Parte<'_>,
    pai: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
    min_rows: usize,
) -> Result<Vec<Line>, String> {
    match p {
        Parte::Elem(c, cs) => layout_min(*c, cs, w, bg, pai, depth + 1, cx, min_rows),
        Parte::Texto(t) => {
            let mut celulas: Vec<Cell> = Vec::new();
            push_text(
                &mut celulas,
                t,
                inline::face(pai, bg, None),
                pai.white,
                pai.transform,
            );
            Ok(wrap(&celulas, w, pai.white)
                .into_iter()
                .map(|l| align(l, w, Align::Left, bg))
                .collect())
        }
    }
}

/// Mede a largura natural de `p` numa largura de teste `avail`. Desfaz os títulos registrados
/// pela medida, porque a renderização definitiva os registra de novo. A medida não centraliza
/// (`margin: auto` e centragem do pai são ignorados), para não contar o recuo de centralização.
fn mede(
    p: &Parte<'_>,
    pai: &Style,
    avail: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<usize, String> {
    let h0 = cx.headings.borrow().len();
    let linhas = match p {
        Parte::Elem(c, cs) => {
            let mut sem_auto = cs.clone();
            sem_auto.margin_auto = false;
            let mut pai_sem_centro = pai.clone();
            pai_sem_centro.align_items = AlignItems::Start;
            layout_min(*c, &sem_auto, avail, bg, &pai_sem_centro, depth + 1, cx, 0)?
        }
        Parte::Texto(_) => renderiza(p, pai, avail, bg, depth, cx)?,
    };
    cx.headings.borrow_mut().truncate(h0);
    Ok(flex_size::largura_natural(&linhas))
}

/// Empilha os itens como blocos, cada um na largura `w`.
fn empilha(
    ps: &[Parte<'_>],
    st: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let mut out: Vec<Line> = Vec::new();
    for p in ps {
        let h0 = cx.headings.borrow().len();
        let ls = renderiza(p, st, w, bg, depth, cx)?;
        desloca(cx, h0, out.len());
        out.extend(ls);
    }
    Ok(out)
}

/// Medidas de um item para o planejamento do flex de linha. A largura-alvo vem de `flex-basis`,
/// de `width` ou, sem nenhum dos dois, da largura natural (medida). O mínimo é a maior palavra
/// (no mínimo `MINIMO_ITEM` colunas), ou o item inteiro quando ele não quebra (controle, imagem,
/// `nowrap`).
fn dimensao(
    p: &Parte<'_>,
    pai: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Dimensao, String> {
    let (grow, explicita, atomico, extra, texto) = match p {
        Parte::Elem(c, cs) => {
            let explicita = cs.flex_basis.or(cs.width).map(|l| medida(l, w));
            // Controles e imagens não quebram: o mínimo é a largura inteira deles.
            let nome = c.value().name();
            let atomico = inline::e_controle(nome)
                || nome == "img"
                || matches!(
                    cs.white,
                    WhiteSpace::NoWrap | WhiteSpace::Pre | WhiteSpace::PreWrap
                );
            (
                cs.flex_grow,
                explicita,
                atomico,
                extras_horizontais(cs),
                inline::texto_visivel(*c, cx)?,
            )
        }
        Parte::Texto(t) => (0.0, None, false, 0, t.clone()),
    };
    let nat = if explicita.is_none() || atomico {
        Some(mede(p, pai, w, bg, depth, cx)?)
    } else {
        None
    };
    let alvo = explicita.or(nat).unwrap_or(0);
    let minimo = if atomico {
        nat.unwrap_or(alvo)
    } else {
        let m = flex_size::maior_palavra(&texto)
            .max(MINIMO_ITEM)
            .saturating_add(extra);
        nat.map_or(m, |n| m.min(n.max(1)))
    };
    Ok(Dimensao {
        alvo,
        minimo,
        grow,
        fixo: grow <= 0.0 && explicita.is_some(),
    })
}

/// Colunas horizontais que a caixa do item gasta fora do conteúdo: preenchimento e bordas laterais.
fn extras_horizontais(cs: &Style) -> usize {
    let bordas = usize::from(cs.border.left.is_some()) + usize::from(cs.border.right.is_some());
    pad(cs.padding[1]) + pad(cs.padding[3]) + 2 * bordas
}

/// Renderiza os itens de uma linha de flex ou grid, cada um na sua largura, e os iguala na altura
/// da linha: a maior altura, no mínimo `alvo`. Com `align-items: stretch`, os itens curtos esticam
/// (a caixa preenche até a altura da linha); com `center` ou `flex-end`, ficam no meio ou na base,
/// com o fundo do contêiner. Devolve uma coluna por item, todas da altura da linha.
fn linha_de_itens(
    ps: &[Parte<'_>],
    larguras: &[usize],
    st: &Style,
    alvo: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Vec<Line>>, String> {
    let h_ini = cx.headings.borrow().len();
    let mut marcas: Vec<usize> = Vec::with_capacity(ps.len() + 1);
    let mut cols: Vec<Vec<Line>> = Vec::with_capacity(ps.len());
    for (p, &lw) in ps.iter().zip(larguras) {
        marcas.push(cx.headings.borrow().len());
        cols.push(renderiza(p, st, lw, bg, depth, cx)?);
    }
    marcas.push(cx.headings.borrow().len());
    let cross = cols.iter().map(Vec::len).max().unwrap_or(0).max(alvo);
    let estica = st.align_items == AlignItems::Stretch;
    if estica && cols.iter().any(|c| c.len() < cross) {
        // Refaz a linha: a caixa curta precisa das linhas extras antes da borda inferior.
        cx.headings.borrow_mut().truncate(h_ini);
        marcas.clear();
        cols.clear();
        for (p, &lw) in ps.iter().zip(larguras) {
            marcas.push(cx.headings.borrow().len());
            cols.push(renderiza_min(p, st, lw, bg, depth, cx, cross)?);
        }
        marcas.push(cx.headings.borrow().len());
    }
    let mut saida = Vec::with_capacity(cols.len());
    for (k, (ls, &lw)) in cols.into_iter().zip(larguras).enumerate() {
        let folga = cross.saturating_sub(ls.len());
        let topo = match st.align_items {
            AlignItems::End => folga,
            AlignItems::Center => folga / 2,
            _ => 0,
        };
        let de = marcas.get(k).copied().unwrap_or(0);
        let ate = marcas.get(k + 1).copied().unwrap_or(0);
        desloca_faixa(cx, de, ate, topo);
        let mut col: Vec<Line> = (0..topo).map(|_| blank(lw, bg)).collect();
        col.extend(ls);
        col.extend((topo..folga).map(|_| blank(lw, bg)));
        saida.push(col);
    }
    Ok(saida)
}

/// Soma `por` às linhas dos títulos registrados entre os índices `de` e `ate`; os demais ficam.
fn desloca_faixa(cx: &Ctx<'_>, de: usize, ate: usize, por: usize) {
    if por == 0 {
        return;
    }
    let mut hs = cx.headings.borrow_mut();
    for h in hs.iter_mut().skip(de).take(ate.saturating_sub(de)) {
        h.0 = h.0.saturating_add(por);
    }
}

/// Flex em linha: lado a lado quando as larguras planejadas cabem (encolhendo até o mínimo de
/// cada item); senão empilha. A sobra vai para quem cresce, ou se distribui pelo `justify`.
fn flex_linha(
    ps: &[Parte<'_>],
    st: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
    alvo: usize,
) -> Result<Vec<Line>, String> {
    let vao = px_to_cols(st.gap);
    let vaos = vao * ps.len().saturating_sub(1);
    let mut dims = Vec::with_capacity(ps.len());
    for p in ps {
        dims.push(dimensao(p, st, w, bg, depth, cx)?);
    }
    let plano = match flex_size::planeja(&dims, vaos, w, st.justify) {
        Some(p) => p,
        None => return empilha(ps, st, w, bg, depth, cx),
    };
    let cols = linha_de_itens(ps, &plano.larguras, st, alvo, bg, depth, cx)?;
    let mut colunas: Vec<Vec<Line>> = Vec::new();
    let mut larguras: Vec<usize> = Vec::new();
    if plano.inicio > 0 {
        colunas.push(Vec::new());
        larguras.push(plano.inicio);
    }
    let ultimo = cols.len().saturating_sub(1);
    for (i, (col, &lw)) in cols.into_iter().zip(plano.larguras.iter()).enumerate() {
        colunas.push(col);
        larguras.push(lw);
        if i < ultimo {
            let vao_total = vao + plano.interno.get(i).copied().unwrap_or(0);
            if vao_total > 0 {
                colunas.push(Vec::new());
                larguras.push(vao_total);
            }
        }
    }
    if plano.fim > 0 {
        colunas.push(Vec::new());
        larguras.push(plano.fim);
    }
    let linhas = side_by_side(colunas, &larguras, 0, bg);
    Ok(linhas.into_iter().map(|l| ajusta(l, w, bg)).collect())
}

/// Flex em coluna: itens empilhados. Com `align-items: center`, cada item fica na sua largura
/// natural, centralizado (só quando `medido`). Gap de 16 px ou mais separa os itens com uma linha
/// em branco. Se o contêiner tem altura (`alvo`), a sobra vai para cima com `flex-end`, se divide
/// com `center` e, sem isso, fica embaixo.
#[allow(clippy::too_many_arguments)] // espelha a assinatura de `flex_lines`, com `medido`
fn flex_coluna(
    ps: &[Parte<'_>],
    st: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
    alvo: usize,
    medido: bool,
) -> Result<Vec<Line>, String> {
    let h_ini = cx.headings.borrow().len();
    let mut out: Vec<Line> = Vec::new();
    let entre = st.gap >= GAP_LINHA_PX;
    let centro = medido && st.align_items == AlignItems::Center;
    for (i, p) in ps.iter().enumerate() {
        if i > 0 && entre {
            out.push(blank(w, bg));
        }
        let h0 = cx.headings.borrow().len();
        let linhas = if centro {
            let nat = mede(p, st, w, bg, depth, cx)?;
            let lead = w.saturating_sub(nat) / 2;
            let mut v: Vec<Line> = Vec::new();
            for l in renderiza(p, st, nat, bg, depth, cx)? {
                let mut nova = blank(lead, bg);
                nova.extend(l);
                v.push(ajusta(nova, w, bg));
            }
            v
        } else {
            renderiza(p, st, w, bg, depth, cx)?
        };
        desloca(cx, h0, out.len());
        out.extend(linhas);
    }
    let folga = alvo.saturating_sub(out.len());
    let topo = match st.justify {
        Justify::End => folga,
        Justify::Center => folga / 2,
        _ => 0,
    };
    desloca(cx, h_ini, topo);
    let mut saida: Vec<Line> = (0..topo).map(|_| blank(w, bg)).collect();
    saida.extend(out);
    saida.extend((topo..folga).map(|_| blank(w, bg)));
    Ok(saida)
}

#[cfg(test)]
mod tests {
    use crate::guard::Limits;
    use crate::model::{line_width, Cell, Face, Line, Rgb};
    use crate::testutil::{render_body, rodar_com, texto};
    use crate::Unlimited;

    use super::side_by_side;

    fn linha_de(s: &str) -> Line {
        s.chars()
            .map(|ch| Cell {
                ch,
                face: Face::default(),
            })
            .collect()
    }

    fn texto_de(l: &Line) -> String {
        l.iter().map(|c| c.ch).collect()
    }

    #[test]
    fn flex_linha_lado_a_lado() {
        let html = "<div style=\"display:flex;gap:16px\"><div>aaa</div><div>bbb</div></div>";
        let l = render_body(html, "", 40);
        assert_eq!(l.len(), 1, "{l:?}");
        assert_eq!(l[0], "aaa  bbb");
        // Em 6 colunas não cabem nem os mínimos: empilha.
        let apertado = render_body(html, "", 6);
        assert_eq!(apertado[0], "aaa");
        assert_eq!(apertado[1], "bbb");
    }

    #[test]
    fn flex_justify_between() {
        let html = "<div style=\"display:flex;justify-content:space-between\">\
            <div>aa</div><div>bb</div></div>";
        let l = render_body(html, "", 10);
        assert_eq!(l[0], "aa      bb");
    }

    #[test]
    fn flex_coluna_center() {
        let html = "<div style=\"display:flex;flex-direction:column;align-items:center\">\
            <div>ab</div><div>abcdef</div></div>";
        let l = render_body(html, "", 10);
        assert_eq!(l[0], "    ab");
        assert_eq!(l[1], "  abcdef");
    }

    #[test]
    fn grid_tres_colunas() {
        let html = "<div style=\"display:grid;grid-template-columns:repeat(3, 1fr)\">\
            <div>1</div><div>2</div><div>3</div><div>4</div><div>5</div><div>6</div></div>";
        let l = render_body(html, "", 60);
        assert_eq!(l[0], format!("{:<20}{:<20}3", "1", "2"));
        assert_eq!(l[1], format!("{:<20}{:<20}6", "4", "5"));
        // Em 20 colunas não cabem três colunas de 12: o grid empilha.
        let estreito = render_body(html, "", 20);
        assert_eq!(estreito[0], "1");
        assert_eq!(estreito[5], "6");
    }

    #[test]
    fn side_by_side_completa_colunas_curtas() {
        let colunas = vec![
            vec![linha_de("ab"), linha_de("cd")],
            vec![linha_de("x")],
        ];
        let linhas = side_by_side(colunas, &[2, 3], 1, None);
        assert_eq!(linhas.len(), 2);
        assert_eq!(texto_de(&linhas[0]), "ab x  ");
        assert_eq!(texto_de(&linhas[1]), "cd    ");
        assert!(linhas.iter().all(|l| line_width(l) == 6));
    }

    #[test]
    fn flex_item_com_fundo_pinta_so_o_item() {
        let html = "<div style=\"display:flex\"><div style=\"background:#222\">oi</div>\
            <div>x</div></div>";
        let saida = rodar_com(html, "", 20, &Limits::default(), &Unlimited).unwrap();
        let linha = &saida.linhas[0];
        assert!(linha[..2]
            .iter()
            .all(|c| c.face.bg == Some(Rgb(34, 34, 34))));
        assert!(linha[2..].iter().all(|c| c.face.bg.is_none()));
    }

    #[test]
    fn nenhuma_linha_passa_da_largura_em_flex_e_grid() {
        let html = "<div style=\"display:flex;gap:16px;justify-content:space-around\">\
            <div>日本語 texto longo</div><span>😀😀</span><div>x</div></div>\
            <div style=\"display:flex;flex-direction:column;align-items:center\">\
            <div style=\"max-width:200px;margin:0 auto\">centro</div>\
            <p>parágrafo longo para quebrar em alguma largura</p></div>\
            <div style=\"display:grid;grid-template-columns:repeat(4,1fr);gap:8px\">\
            <div>a</div><div>bbbbbbbbbbbbbbbbbbbbbbbb</div><div>c</div><div>d</div><div>e</div>\
            </div>";
        for w in [20usize, 40, 80] {
            let saida = rodar_com(html, "", w, &Limits::default(), &Unlimited).unwrap();
            assert!(!saida.linhas.is_empty());
            for l in &saida.linhas {
                assert_eq!(line_width(l), w, "largura {w}: {:?}", texto(l));
            }
        }
    }

    #[test]
    fn flex_aninhado_fundo_termina_rapido() {
        // Cada nível mede o item e o renderiza de novo: sem teto de elementos seria 2^n.
        let n = 40;
        let html = format!(
            "{}fim{}",
            "<div style=\"display:flex\"><div>".repeat(n),
            "</div></div>".repeat(n)
        );
        let lim = Limits { max_elements: 5_000, ..Default::default() };
        let t = std::time::Instant::now();
        let r = rodar_com(&html, "", 80, &lim, &Unlimited);
        assert!(t.elapsed().as_secs() < 5, "demorou {:?}", t.elapsed());
        match r {
            Ok(s) => assert!(s.linhas.iter().any(|l| texto(l).contains("fim"))),
            Err(e) => assert!(e.contains("complexa demais"), "{e}"),
        }
    }

    /// Cor de fundo da célula `col` da linha `l`.
    fn fundo(l: &Line, col: usize) -> Option<Rgb> {
        l.get(col).and_then(|c| c.face.bg)
    }

    /// Índices das linhas cuja célula `col` tem fundo `cor`.
    fn linhas_com_fundo(linhas: &[Line], col: usize, cor: Rgb) -> Vec<usize> {
        linhas
            .iter()
            .enumerate()
            .filter(|(_, l)| fundo(l, col) == Some(cor))
            .map(|(k, _)| k)
            .collect()
    }

    fn saida(html: &str, w: usize) -> Vec<Line> {
        rodar_com(html, "", w, &Limits::default(), &Unlimited).unwrap().linhas
    }

    const ESCURO: Rgb = Rgb(34, 34, 34);
    const MEIO: Rgb = Rgb(51, 51, 51);

    #[test]
    fn shrink_em_vez_de_empilhar() {
        // Naturais de ~77 colunas cada: em 100 colunas encolhem e ficam lado a lado.
        let corpo = " um dois tres quatro cinco seis sete oito nove dez onze doze treze";
        let html = format!(
            "<div style=\"display:flex;gap:16px\"><div>primeiro{corpo}</div>\
             <div>segundo{corpo}</div><div>terceiro{corpo}</div></div>"
        );
        let l = saida(&html, 100);
        assert!(l.len() >= 3, "{l:?}");
        let primeira = texto(&l[0]);
        assert!(primeira.contains("primeiro"), "{primeira:?}");
        assert!(primeira.contains("segundo"), "{primeira:?}");
        assert!(primeira.contains("terceiro"), "{primeira:?}");
        assert!(l.iter().all(|x| line_width(x) == 100));
    }

    #[test]
    fn empilha_quando_minimos_nao_cabem() {
        let html = format!(
            "<div style=\"display:flex;gap:16px\">{}</div>",
            "<div>alfa beta</div>".repeat(6)
        );
        let l = saida(&html, 20);
        assert_eq!(l.len(), 6, "{l:?}");
        assert!(l.iter().all(|x| texto(x).matches("alfa").count() <= 1));
    }

    #[test]
    fn flex_grow_divide_a_sobra() {
        let html = "<div style=\"display:flex\"><div style=\"flex:1;background:#222\">a</div>\
            <div style=\"flex:2;background:#333\">b</div></div>";
        let l = saida(html, 30);
        assert_eq!(l.len(), 1, "{l:?}");
        for c in 0..10 {
            assert_eq!(fundo(&l[0], c), Some(ESCURO), "coluna {c}");
        }
        for c in 10..30 {
            assert_eq!(fundo(&l[0], c), Some(MEIO), "coluna {c}");
        }
    }

    #[test]
    fn flex_basis_px_e_largura_fixa() {
        // 160 px = 20 colunas fixas; o segundo item fica com o resto.
        let html = "<div style=\"display:flex\"><div style=\"flex:0 0 160px;background:#222\">a</div>\
            <div style=\"flex:1;background:#333\">b</div></div>";
        let l = saida(html, 40);
        for c in 0..20 {
            assert_eq!(fundo(&l[0], c), Some(ESCURO), "coluna {c}");
        }
        for c in 20..40 {
            assert_eq!(fundo(&l[0], c), Some(MEIO), "coluna {c}");
        }
    }

    #[test]
    fn menu_lateral_com_flex_1() {
        let longo = "palavra ".repeat(20);
        let html = format!(
            "<div style=\"display:flex\"><aside style=\"width:220px\">Menu</aside>\
             <main style=\"flex:1\">{longo}</main></div>"
        );
        let l = saida(&html, 100);
        assert!(texto(&l[0]).starts_with("Menu"), "{:?}", texto(&l[0]));
        // 220 px = 28 colunas: o conteúdo começa na coluna 28, na primeira e nas demais linhas.
        assert_eq!(texto(&l[0]).find("palavra"), Some(28));
        assert_eq!(texto(&l[1]).find("palavra"), Some(28));
        assert!(l.len() >= 2);
        assert!(l.iter().all(|x| line_width(x) == 100));
    }

    #[test]
    fn alturas_iguais_em_flex() {
        let html = "<div style=\"display:flex\"><div style=\"background:#222\">a<br>b<br>c</div>\
            <div style=\"background:#333\">x</div></div>";
        let l = saida(html, 20);
        assert_eq!(l.len(), 3, "{:?}", l.iter().map(|x| texto(x)).collect::<Vec<_>>());
        for (k, linha) in l.iter().enumerate() {
            assert_eq!(fundo(linha, 0), Some(ESCURO), "linha {k}");
            assert_eq!(fundo(linha, 1), Some(MEIO), "linha {k}");
        }
    }

    #[test]
    fn alturas_iguais_em_grid_com_borda() {
        let html = "<div style=\"display:grid;grid-template-columns:repeat(2,1fr)\">\
            <div style=\"border:1px solid #888;background:#222\">a</div>\
            <div style=\"border:1px solid #888;background:#333\">b<br>c<br>d</div></div>";
        let l = saida(html, 40);
        assert_eq!(l.len(), 5, "{:?}", l.iter().map(|x| texto(x)).collect::<Vec<_>>());
        // A borda inferior do item curto cai na mesma linha da do item alto: cantos `└` e `┘`
        // nas pontas da caixa e `─` entre eles.
        assert_eq!(l[4].first().map(|x| x.ch), Some('└'));
        assert_eq!(l[4].get(19).map(|x| x.ch), Some('┘'));
        for c in 1..19 {
            assert_eq!(l[4].get(c).map(|x| x.ch), Some('─'), "coluna {c}");
        }
        for c in 21..39 {
            assert_eq!(l[4].get(c).map(|x| x.ch), Some('─'), "coluna {c}");
        }
        assert_eq!(l[4].get(20).map(|x| x.ch), Some('└'));
        assert_eq!(l[4].get(39).map(|x| x.ch), Some('┘'));
    }

    #[test]
    fn align_items_center() {
        let html = "<div style=\"display:flex;align-items:center;background:#111\">\
            <div style=\"background:#222;height:32px\">a</div>\
            <div style=\"background:#333;height:96px\">b</div></div>";
        let l = saida(html, 20);
        assert_eq!(l.len(), 6);
        // O item de 2 linhas fica no meio das 6: linhas 2 e 3.
        assert_eq!(linhas_com_fundo(&l, 0, ESCURO), vec![2, 3]);
        // As sobras são preenchidas com o fundo do contêiner.
        assert_eq!(fundo(&l[0], 0), Some(Rgb(17, 17, 17)));
        assert_eq!(fundo(&l[5], 0), Some(Rgb(17, 17, 17)));
    }

    #[test]
    fn align_items_end_com_barras() {
        let html = "<div style=\"display:flex;align-items:flex-end;gap:8px\">\
            <div style=\"background:#222;height:32px\"></div>\
            <div style=\"background:#333;height:64px\"></div>\
            <div style=\"background:#444;height:96px\"></div></div>";
        let l = saida(html, 20);
        assert_eq!(l.len(), 6);
        assert_eq!(linhas_com_fundo(&l, 0, ESCURO), vec![4, 5]);
        assert_eq!(linhas_com_fundo(&l, 2, MEIO), vec![2, 3, 4, 5]);
        assert_eq!(linhas_com_fundo(&l, 4, Rgb(68, 68, 68)), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn barras_de_coluna_crescem_da_base() {
        // Contêiner de 80 px (5 linhas); cada coluna tem uma barra com altura em %.
        let coluna = |cor: &str, pct: u32| {
            format!(
                "<div style=\"flex:1;display:flex;flex-direction:column;justify-content:flex-end;\
                 height:100%\"><div style=\"height:{pct}%;background:{cor}\"></div></div>"
            )
        };
        let html = format!(
            "<div style=\"display:flex;align-items:flex-end;height:80px;gap:8px\">{}{}</div>",
            coluna("#222", 40),
            coluna("#333", 80)
        );
        let l = saida(&html, 20);
        assert_eq!(l.len(), 5, "{:?}", l.iter().map(|x| texto(x)).collect::<Vec<_>>());
        assert_eq!(linhas_com_fundo(&l, 0, ESCURO), vec![3, 4]);
        let col_b = (0..20).find(|&c| fundo(&l[4], c) == Some(MEIO)).unwrap_or(usize::MAX);
        assert_eq!(linhas_com_fundo(&l, col_b, MEIO), vec![1, 2, 3, 4]);
    }

    #[test]
    fn largura_total_exata() {
        let html = "<div style=\"display:flex;gap:16px\"><div>alfa beta gama delta epsilon zeta \
            eta theta iota</div><div style=\"flex:1\">kappa lambda mu nu xi omicron pi rho sigma \
            tau</div><div style=\"width:20px\">upsilon phi chi psi omega</div></div>";
        for w in [20usize, 40, 80, 120] {
            let l = saida(html, w);
            assert!(!l.is_empty());
            for x in &l {
                assert_eq!(line_width(x), w, "largura {w}: {:?}", texto(x));
            }
        }
    }
}
