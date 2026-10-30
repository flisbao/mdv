//! Layout de blocos: largura e margens da caixa, bordas, preenchimento, percurso dos filhos,
//! títulos e listas. Cada chamada devolve linhas de exatamente `avail` colunas.
//! Flex e grid ficam em `flex.rs`; tabelas, em `table.rs`.

use crate::boxes::{caixa_de_bloco, linha_borda, monta, redondo, Caixa};
use crate::ctx::Ctx;
use crate::flex;
use crate::inline::{self, Item};
use crate::length::{px_to_cols, px_to_rows, Len};
use crate::list;
use crate::model::{line_width, Cell, Face, Line, Rgb};
use crate::pre;
use crate::selector::Pseudo;
use crate::style::{compute, compute_pseudo, Align, AlignItems, Display, Marker, Style, Transform, WhiteSpace};
use crate::table;
use crate::text::{align, blank, push_text, truncate, wrap};
use crate::ua;
use scraper::ElementRef;

/// Recuo máximo, em colunas, de cada lado do preenchimento.
const PADDING_MAX: usize = 8;
/// Largura mínima de uma caixa de bloco, em colunas (quando cabe no espaço).
const CAIXA_MIN: usize = 4;

/// Layout de um bloco `el` com estilo `st` na largura `avail`. `paint` é o fundo atrás da caixa,
/// `parent` o estilo do pai e `depth` a profundidade de blocos (acima de `max_walk_depth` o
/// conteúdo é achatado em texto).
pub fn layout(
    el: ElementRef<'_>,
    st: &Style,
    avail: usize,
    paint: Option<Rgb>,
    parent: &Style,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    layout_min(el, st, avail, paint, parent, depth, cx, 0)
}

/// Como `layout`, mas a caixa tem pelo menos `min_rows` linhas no total (bordas e preenchimento
/// incluídos). É como um item de flex ou de grid se estica até a altura da sua linha: o fundo e
/// as bordas laterais seguem nas linhas extras, e a borda inferior fica por último.
#[allow(clippy::too_many_arguments)] // `layout` com `min_rows`: o mesmo conjunto de `layout`
pub(crate) fn layout_min(
    el: ElementRef<'_>,
    st: &Style,
    avail: usize,
    paint: Option<Rgb>,
    parent: &Style,
    depth: usize,
    cx: &Ctx<'_>,
    min_rows: usize,
) -> Result<Vec<Line>, String> {
    if st.display == Display::None {
        return Ok(Vec::new());
    }
    if depth > cx.cascade.lim.max_walk_depth {
        return achatado(el, st, avail, paint, cx);
    }
    let nome = el.value().name();
    let my_bg = inline::fundo_efetivo(st, paint);
    let w = largura_caixa(st, avail);
    let centra = st.margin_auto
        || (parent.display == Display::Flex
            && parent.flex_column
            && parent.align_items == AlignItems::Center);
    let lo = if centra && w < avail { (avail - w) / 2 } else { 0 };
    let ro = avail - w - lo;
    // A borda de uma tabela é desenhada pela grade (`table.rs`), não pela caixa.
    let tabela = st.display == Display::Table;
    // Listas: o marcador pende na área do item, então a lista não recua pelo padding.
    let (pl, pr) = if matches!(nome, "ul" | "ol") {
        (0, 0)
    } else {
        (pad(st.padding[3]), pad(st.padding[1]))
    };
    // Bordas laterais só entram se sobra ao menos uma coluna de conteúdo; senão somem.
    let quer_esq = !tabela && st.border.left.is_some();
    let quer_dir = !tabela && st.border.right.is_some();
    let laterais = 2 * (usize::from(quer_esq) + usize::from(quer_dir));
    let cabe = w > laterais + pl + pr;
    let esq = if quer_esq && cabe { st.border.left } else { None };
    let dir = if quer_dir && cabe { st.border.right } else { None };
    let bl = if esq.is_some() { 2 } else { 0 };
    let br = if dir.is_some() { 2 } else { 0 };
    let inner_w = w.saturating_sub(bl + br + pl + pr).max(1);
    // Topo e base: a caixa desenha as duas; os demais elementos (exceto `hr`) só com fundo.
    let caixa_bloco = !tabela && caixa_de_bloco(nome, st);
    let sup = if !tabela && (caixa_bloco || nome == "hr" || my_bg.is_some()) {
        st.border.top
    } else {
        None
    };
    let inf = if !tabela && (caixa_bloco || my_bg.is_some()) {
        st.border.bottom
    } else {
        None
    };
    let caixa = Caixa {
        avail,
        lo,
        ro,
        pl,
        pr,
        paint,
        bg: my_bg,
        esq,
        dir,
        sup,
        inf,
        caixa: caixa_bloco,
        redondo: caixa_bloco && redondo(st),
    };

    // Bordas e preenchimento verticais, que ocupam linhas fora do conteúdo.
    let borda_sup = sup.is_some();
    let borda_inf = inf.is_some();
    let pt = st.padding[0] >= 12.0;
    let pb = st.padding[2] >= 12.0;
    let extras = usize::from(borda_sup) + usize::from(borda_inf) + usize::from(pt) + usize::from(pb);
    let minimo_interno = min_rows.saturating_sub(extras);

    let h0 = cx.headings.borrow().len();
    let mut interno = if nome == "pre" {
        pre::pre_lines(el, st, inner_w, my_bg, cx)?
    } else if inline::e_controle(nome) {
        controle_linhas(el, st, parent, paint, inner_w, my_bg, cx)?
    } else {
        match st.display {
            Display::Flex => {
                let alvo_flex = linhas_minimas(st, false).max(minimo_interno);
                flex::flex_lines(el, st, inner_w, my_bg, depth, cx, alvo_flex)?
            }
            Display::Grid => flex::grid_lines(el, st, inner_w, my_bg, depth, cx)?,
            Display::Table => table::table_lines(el, st, inner_w, my_bg, depth, cx)?,
            _ => conteudo(el, st, inner_w, my_bg, depth, cx)?,
        }
    };
    // Altura CSS: completa o conteúdo com linhas de fundo (uma barra vazia com fundo ocupa a altura).
    let alvo = linhas_minimas(st, interno.is_empty() && my_bg.is_some()).max(minimo_interno);
    while interno.len() < alvo {
        interno.push(blank(inner_w, my_bg));
    }
    let titulo = nivel_titulo(nome).map(|n| (n, texto_linhas(&interno)));

    if pt {
        interno.insert(0, blank(inner_w, my_bg));
    }
    if pb {
        interno.push(blank(inner_w, my_bg));
    }
    desloca(cx, h0, usize::from(borda_sup) + usize::from(pt));

    let mut out: Vec<Line> = Vec::new();
    if borda_sup {
        out.push(linha_borda(&caixa, true));
    }
    for l in interno {
        out.push(monta(&caixa, l));
    }
    if borda_inf {
        out.push(linha_borda(&caixa, false));
    }
    if st.margin_bottom >= 14.0 && !out.is_empty() {
        out.push(blank(avail, paint));
    }
    if let Some((nivel, texto)) = titulo {
        if !texto.is_empty() {
            cx.headings.borrow_mut().push((0, nivel, texto));
        }
    }
    Ok(out)
}

/// Largura da caixa: `width` em px ou %, com `min-width` como piso e `max-width` como teto, limitada
/// a `avail` (mínimo 4 colunas).
fn largura_caixa(st: &Style, avail: usize) -> usize {
    let mut w = avail;
    if let Some(l) = st.width {
        w = medida(l, avail);
    }
    if let Some(m) = st.min_width {
        w = w.max(medida(m, avail));
    }
    if let Some(m) = st.max_width {
        w = w.min(medida(m, avail));
    }
    w.clamp(avail.min(CAIXA_MIN), avail)
}

pub(crate) fn medida(l: Len, avail: usize) -> usize {
    match l {
        Len::Px(px) => px_to_cols(px),
        Len::Percent(p) => ((avail as f32) * p / 100.0)
            .round()
            .clamp(0.0, avail as f32) as usize,
        Len::Auto => avail,
    }
}

/// Linhas mínimas do conteúdo pela altura CSS: `height` ou `min-height` em px (16 px por linha, no
/// mínimo 1 quando a altura é positiva). `height` em % só vale para bloco vazio com fundo: ocupa
/// uma linha, como a barra de progresso feita de `div`.
fn linhas_minimas(st: &Style, vazio_com_fundo: bool) -> usize {
    let de_px = |px: f32| if px > 0.0 { px_to_rows(px).max(1) } else { 0 };
    let altura = match st.height {
        Some(Len::Px(px)) => de_px(px),
        Some(Len::Percent(p)) if p > 0.0 && vazio_com_fundo => 1,
        _ => 0,
    };
    let minima = match st.min_height {
        Some(Len::Px(px)) => de_px(px),
        _ => 0,
    };
    altura.max(minima)
}

/// Linhas de um elemento de linha (controle, gráfico ou mídia) quando ele é o próprio bloco, como
/// item de flex ou grid: o elemento é emitido por `inline::elemento`, quebrado na largura `w`.
fn controle_linhas(
    el: ElementRef<'_>,
    st: &Style,
    pai: &Style,
    paint: Option<Rgb>,
    w: usize,
    bg: Option<Rgb>,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let mut run: Vec<Cell> = Vec::new();
    cx.largura.set(w);
    inline::elemento(el, st.clone(), pai, paint, cx, &mut run)?;
    let mut out = Vec::new();
    descarrega(&mut run, &mut out, w, st, bg, cx)?;
    Ok(out)
}

/// Preenchimento horizontal em colunas, limitado a `PADDING_MAX`.
pub(crate) fn pad(px: f32) -> usize {
    px_to_cols(px).min(PADDING_MAX)
}

/// Filhos de `el` empilhados em blocos e texto corrido; cada linha de bloco é `w` colunas.
pub(crate) fn conteudo(
    el: ElementRef<'_>,
    st: &Style,
    w: usize,
    bg: Option<Rgb>,
    depth: usize,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let mut out: Vec<Line> = Vec::new();
    let mut run: Vec<Cell> = Vec::new();
    let nivel = if st.list_marker == Marker::None {
        0
    } else {
        list::nivel(el, cx.cascade.lim)
    };
    let mut n = list::inicio(el);
    if let Some(marca) = ua::marca_detalhes(el) {
        push_text(&mut run, marca, inline::face(st, bg, None), st.white, st.transform);
    }
    antes_depois(el, st, Pseudo::Before, w, bg, cx, &mut out, &mut run)?;
    for item in inline::filhos(el) {
        match item {
            Item::Texto(t) => {
                push_text(&mut run, &t, inline::face(st, bg, None), st.white, st.transform)
            }
            Item::Elem(c) => {
                cx.tick()?;
                let cs = compute(c, st, cx.cascade);
                if inline::oculto(&cs) {
                    continue;
                }
                let nome = c.value().name();
                if nome == "br" {
                    run.push(inline::celula('\n', Face::default()));
                    continue;
                }
                if inline::em_linha(nome, &cs) {
                    cx.largura.set(w);
                    inline::elemento(c, cs, st, bg, cx, &mut run)?;
                    continue;
                }
                descarrega(&mut run, &mut out, w, st, bg, cx)?;
                let h0 = cx.headings.borrow().len();
                if nome == "li" {
                    let marca = list::marca(st.list_marker, nivel, n).map(|t| {
                        let mut v = Vec::new();
                        let f = inline::face(&cs, bg, None);
                        push_text(&mut v, &format!("{t} "), f, WhiteSpace::Normal, Transform::None);
                        v
                    });
                    n = n.saturating_add(1);
                    let ind = marca.as_ref().map_or(0, |m| line_width(m));
                    let linhas = layout(c, &cs, w.saturating_sub(ind), bg, st, depth + 1, cx)?;
                    desloca(cx, h0, out.len());
                    out.extend(prefixa(linhas, marca, bg, w));
                } else {
                    let linhas = layout(c, &cs, w, bg, st, depth + 1, cx)?;
                    desloca(cx, h0, out.len());
                    out.extend(linhas);
                }
            }
        }
    }
    antes_depois(el, st, Pseudo::After, w, bg, cx, &mut out, &mut run)?;
    descarrega(&mut run, &mut out, w, st, bg, cx)?;
    Ok(out)
}

/// Pseudoelemento `qual` de `el` (no bloco `st`): se é de bloco, vira linhas próprias antes ou
/// depois do conteúdo; se é de linha, entra no texto corrido `run`.
#[allow(clippy::too_many_arguments)] // mesmo conjunto de `conteudo`, mais a saída e o texto corrido
fn antes_depois(
    el: ElementRef<'_>,
    st: &Style,
    qual: Pseudo,
    w: usize,
    bg: Option<Rgb>,
    cx: &Ctx<'_>,
    out: &mut Vec<Line>,
    run: &mut Vec<Cell>,
) -> Result<(), String> {
    let Some(ps) = compute_pseudo(el, st, qual, cx.cascade) else {
        return Ok(());
    };
    if ps.display == Display::None {
        return Ok(());
    }
    let texto = ps.content.as_deref().unwrap_or("");
    let fbg = inline::fundo_efetivo(&ps, bg);
    if matches!(
        ps.display,
        Display::Block | Display::ListItem | Display::Flex | Display::Grid | Display::Table
    ) {
        descarrega(run, out, w, st, bg, cx)?;
        out.extend(bloco_pseudo(&ps, texto, w, fbg, cx)?);
    } else {
        push_text(run, texto, inline::face(&ps, fbg, None), ps.white, ps.transform);
    }
    Ok(())
}

/// Linhas de um pseudoelemento de bloco: o texto quebrado em `w` colunas; sem texto, uma barra
/// de fundo com a altura CSS (`height`/`min-height`).
fn bloco_pseudo(
    ps: &Style,
    texto: &str,
    w: usize,
    bg: Option<Rgb>,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let mut run: Vec<Cell> = Vec::new();
    push_text(&mut run, texto, inline::face(ps, bg, None), ps.white, ps.transform);
    let mut out = Vec::new();
    descarrega(&mut run, &mut out, w, ps, bg, cx)?;
    if out.is_empty() {
        let alvo = linhas_minimas(ps, bg.is_some()).max(usize::from(bg.is_some()));
        cx.linhas(alvo)?;
        out.extend((0..alvo).map(|_| blank(w, bg)));
    }
    Ok(out)
}

/// Põe o marcador na primeira linha do item e recuo (largura do marcador) nas demais.
fn prefixa(linhas: Vec<Line>, marca: Option<Line>, bg: Option<Rgb>, w: usize) -> Vec<Line> {
    let ind = marca.as_ref().map_or(0, |m| line_width(m));
    let mut marca = marca;
    let mut linhas = linhas;
    if linhas.is_empty() && marca.is_some() {
        linhas.push(Vec::new());
    }
    linhas
        .into_iter()
        .map(|l| {
            let mut nova = match marca.take() {
                Some(m) => m,
                None => blank(ind, bg),
            };
            nova.extend(l);
            ajusta(nova, w, bg)
        })
        .collect()
}

/// Quebra o texto corrido acumulado em linhas de `w` colunas, com o alinhamento do bloco.
fn descarrega(
    run: &mut Vec<Cell>,
    out: &mut Vec<Line>,
    w: usize,
    st: &Style,
    bg: Option<Rgb>,
    cx: &Ctx<'_>,
) -> Result<(), String> {
    if run.is_empty() {
        return Ok(());
    }
    let linhas = wrap(run, w, st.white);
    run.clear();
    cx.linhas(linhas.len())?;
    for l in linhas {
        let l = if st.white == WhiteSpace::NoWrap {
            truncate(&l, w)
        } else {
            l
        };
        out.push(align(l, w, st.align, bg));
    }
    Ok(())
}

/// Bloco achatado: todo o conteúdo como texto corrido, sem recursão de blocos.
fn achatado(
    el: ElementRef<'_>,
    st: &Style,
    avail: usize,
    paint: Option<Rgb>,
    cx: &Ctx<'_>,
) -> Result<Vec<Line>, String> {
    let bg = inline::fundo_efetivo(st, paint);
    cx.largura.set(avail);
    let mut run = inline::celulas_filhos(el, st, bg, cx)?;
    let mut out = Vec::new();
    descarrega(&mut run, &mut out, avail, st, bg, cx)?;
    Ok(out.into_iter().map(|l| ajusta(l, avail, paint)).collect())
}

/// Corta a linha se passar de `w` colunas e completa com espaços até `w`.
pub(crate) fn ajusta(l: Line, w: usize, bg: Option<Rgb>) -> Line {
    let l = if line_width(&l) > w { truncate(&l, w) } else { l };
    align(l, w, Align::Left, bg)
}

/// Nível do título (`h1` a `h6`).
fn nivel_titulo(nome: &str) -> Option<u8> {
    match nome {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Texto das linhas, aparado e com as linhas não vazias unidas por espaço.
fn texto_linhas(linhas: &[Line]) -> String {
    linhas
        .iter()
        .map(|l| l.iter().map(|c| c.ch).collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Soma `por` às linhas dos títulos registrados a partir do índice `desde`.
pub(crate) fn desloca(cx: &Ctx<'_>, desde: usize, por: usize) {
    if por == 0 {
        return;
    }
    for h in cx.headings.borrow_mut().iter_mut().skip(desde) {
        h.0 = h.0.saturating_add(por);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Limits;
    use crate::model::line_width;
    use crate::Unlimited;
    use crate::testutil::{faces, render_body, rodar_com, texto};

    #[test]
    fn summary_fechado_e_aberto_tem_seta() {
        let fechado = render_body("<details><summary>Mais</summary><p>texto</p></details>", "", 40);
        assert_eq!(fechado[0], "▸ Mais", "{fechado:?}");
        assert!(fechado.iter().any(|l| l == "texto"), "{fechado:?}");
        let aberto = render_body("<details open><summary>Mais</summary><p>texto</p></details>", "", 40);
        assert_eq!(aberto[0], "▾ Mais", "{aberto:?}");
    }

    #[test]
    fn summary_fora_de_details_nao_ganha_seta() {
        let l = render_body("<div><summary>x</summary></div>", "", 40);
        assert_eq!(l[0], "x", "{l:?}");
    }

    #[test]
    fn dd_recua_4_colunas() {
        let l = render_body("<dl><dt>Termo</dt><dd>Def</dd></dl>", "", 40);
        let linha = l.iter().find(|s| s.contains("Def")).cloned().unwrap_or_default();
        assert!(linha.starts_with("    Def"), "{l:?}");
        assert!(!linha.starts_with("     "), "{l:?}");
    }

    #[test]
    fn paragrafos_e_margem() {
        let l = render_body("<p>um</p><p>dois</p>", "", 20);
        assert_eq!(l, vec!["um", "", "dois", ""]);
    }

    #[test]
    fn inline_em_uma_linha() {
        let saida = rodar_com(
            "<p>a <b>b</b> <i>c</i></p>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(texto(&saida.linhas[0]), "a b c");
        assert!(faces(&saida.linhas, 'b')[0].bold);
        assert!(!faces(&saida.linhas, 'a')[0].bold);
        assert!(faces(&saida.linhas, 'c')[0].italic);
        assert!(!faces(&saida.linhas, 'b')[0].italic);
    }

    #[test]
    fn link_marca_face() {
        let saida = rodar_com(
            "<a href=\"x\">oi</a> <a href=\"x\">ei</a> <a href=\"y\">ai</a>",
            "",
            30,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(faces(&saida.linhas, 'o')[0].link, Some(0));
        assert_eq!(faces(&saida.linhas, 'e')[0].link, Some(0));
        assert_eq!(faces(&saida.linhas, 'a')[0].link, Some(1));
        assert_eq!(saida.links, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn lista_com_marcadores() {
        let ul = render_body("<ul><li>a</li><li>b</li></ul>", "", 20);
        assert_eq!(ul[0], "• a");
        assert_eq!(ul[1], "• b");
        let ol = render_body("<ol start=3><li>a</li><li>b</li></ol>", "", 20);
        assert_eq!(ol[0], "3. a");
        assert_eq!(ol[1], "4. b");
        let aninhada = render_body("<ul><li>a<ul><li>b</li></ul></li></ul>", "", 20);
        assert_eq!(aninhada[0], "• a");
        assert_eq!(aninhada[1], "  ◦ b");
    }

    #[test]
    fn br_quebra() {
        let l = render_body("<p>a<br>b</p>", "", 20);
        assert_eq!(&l[..2], &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn texto_escondido_nunca_aparece() {
        let html = "<p style=\"display:none\">A</p><p hidden>B</p><p style=\"opacity:0\">C</p>\
            <p style=\"visibility:hidden\">D</p><script>E</script><style>F</style>\
            <template>G</template><noscript>H</noscript><p>ok</p>";
        let l = render_body(html, "", 20);
        let todo = l.join("\n");
        for ch in ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'] {
            assert!(!todo.contains(ch), "apareceu {ch}: {todo:?}");
        }
        assert!(todo.contains("ok"));
    }

    #[test]
    fn cor_do_css() {
        let saida = rodar_com(
            "<p>oi</p>",
            "p{color:#d97757}",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert!(faces(&saida.linhas, 'o')
            .iter()
            .all(|f| f.fg == Some(Rgb(217, 119, 87))));
    }

    #[test]
    fn fundo_pinta_a_largura_toda() {
        let saida = rodar_com(
            "<div>oi</div>",
            "div{background:#222}",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let linha = &saida.linhas[0];
        assert_eq!(linha.len(), 10);
        assert!(linha.iter().all(|c| c.face.bg == Some(Rgb(34, 34, 34))));
    }

    #[test]
    fn centraliza_com_text_align() {
        let l = render_body("<p style=\"text-align:center\">oi</p>", "", 10);
        assert_eq!(l[0], "    oi");
    }

    #[test]
    fn max_width_centralizado_com_margin_auto() {
        let l = render_body(
            "<div style=\"max-width:320px;margin:0 auto\">oi</div>",
            "",
            80,
        );
        assert_eq!(l[0].chars().position(|c| c == 'o'), Some(20));
    }

    #[test]
    fn borda_esquerda_vira_barra() {
        let saida = rodar_com(
            "<div style=\"border-left:3px solid #f00\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(texto(&saida.linhas[0]), "▌ oi");
        assert_eq!(faces(&saida.linhas, '▌')[0].fg, Some(Rgb(255, 0, 0)));
    }

    #[test]
    fn pre_preserva_e_trunca() {
        let longo = "x".repeat(100);
        let saida = rodar_com(
            &format!("<pre>{longo}</pre>"),
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let primeira = texto(&saida.linhas[0]);
        assert!(primeira.starts_with(' '), "{primeira:?}");
        assert!(primeira.ends_with('…'), "{primeira:?}");
        assert!(saida.linhas.iter().all(|l| line_width(l) == 20));
    }

    #[test]
    fn pre_com_span_colorido() {
        let saida = rodar_com(
            "<pre><span style=\"color:red\">x</span> y</pre>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(faces(&saida.linhas, 'x')[0].fg, Some(Rgb(255, 0, 0)));
        // Sem cor própria, o texto do pre herda o padrão (corrigido pelo contraste), não o vermelho.
        assert_ne!(faces(&saida.linhas, 'y')[0].fg, Some(Rgb(255, 0, 0)));
    }

    #[test]
    fn hr_vira_regua() {
        let l = render_body("<hr>", "", 10);
        assert_eq!(l[0], "─".repeat(10));
    }

    #[test]
    fn img_alt() {
        let l = render_body("<p><img alt=\"logo\"></p>", "", 30);
        assert!(l[0].contains("[imagem: logo]"), "{l:?}");
    }

    #[test]
    fn input_e_button() {
        let l = render_body(
            "<p><input type=\"checkbox\" checked> <button>Ir</button></p>",
            "",
            30,
        );
        assert!(l[0].contains("[x]"), "{l:?}");
        assert!(l[0].contains("[ Ir ]"), "{l:?}");
    }

    #[test]
    fn profundidade_achata() {
        let mut html = String::new();
        for _ in 0..10 {
            html.push_str("<div>");
        }
        html.push_str("fim");
        for _ in 0..10 {
            html.push_str("</div>");
        }
        let lim = Limits {
            max_walk_depth: 3,
            ..Default::default()
        };
        let saida = rodar_com(&html, "", 20, &lim, &Unlimited).unwrap();
        let todo: String = saida.linhas.iter().map(|l| texto(l)).collect::<Vec<_>>().join("\n");
        assert!(todo.contains("fim"), "{todo:?}");
    }

    #[test]
    fn elementos_demais_e_erro() {
        let lim = Limits {
            max_elements: 5,
            ..Default::default()
        };
        let html = "<p>x</p>".repeat(20);
        let err = rodar_com(&html, "", 20, &lim, &Unlimited).err().unwrap();
        assert!(err.contains("complexa demais"), "{err}");
    }

    #[test]
    fn nenhuma_linha_passa_da_largura() {
        let html = "<h1>Título bem comprido de teste</h1><p>日本語 😀😀 texto texto texto \
            texto texto texto texto texto texto texto</p><ul><li>item um comprido demais \
            para caber</li><li>dois</li></ul><pre>linha de código muito muito muito longa</pre>\
            <div style=\"border-left:3px solid #f00;padding:8px\">caixa com borda e padding \
            que deve quebrar</div><hr>";
        for w in [20usize, 40, 80] {
            let saida = rodar_com(html, "", w, &Limits::default(), &Unlimited).unwrap();
            for l in &saida.linhas {
                assert_eq!(line_width(l), w, "largura {w}: {:?}", texto(l));
            }
        }
    }

    #[test]
    fn profundidade_e_cadeia_de_spans_nao_estouram() {
        // Aninhamento acima de max_walk_depth e cadeia longa de spans: termina sem recursão profunda.
        let mut html = String::from("<p>");
        html.push_str(&"<span>".repeat(5000));
        html.push('x');
        html.push_str(&"</span>".repeat(5000));
        html.push_str("</p>");
        html.push_str(&"<div>".repeat(1000));
        html.push_str("fundo");
        html.push_str(&"</div>".repeat(1000));
        let saida = rodar_com(&html, "", 30, &Limits::default(), &Unlimited).unwrap();
        let todo: String = saida.linhas.iter().map(|l| texto(l)).collect::<Vec<_>>().join("\n");
        assert!(todo.contains('x') && todo.contains("fundo"), "{todo:?}");
    }

    /// Células de `linhas` cujo fundo é `cor`.
    fn com_fundo(linhas: &[Line], cor: Rgb) -> usize {
        linhas
            .iter()
            .flat_map(|l| l.iter())
            .filter(|c| c.face.bg == Some(cor))
            .count()
    }

    #[test]
    fn height_px_completa_com_linhas_de_fundo() {
        let saida = rodar_com(
            "<div style=\"background:#222;height:48px\">oi</div>",
            "",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(saida.linhas.len(), 3);
        assert_eq!(com_fundo(&saida.linhas[1..], Rgb(34, 34, 34)), 20);
    }

    #[test]
    fn min_height_px_vale_como_piso() {
        let saida = rodar_com(
            "<div style=\"min-height:32px;background:#222\"></div>",
            "",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(saida.linhas.len(), 2);
    }

    #[test]
    fn height_em_bloco_sem_fundo_ocupa_linhas() {
        let l = render_body("<div style=\"height:32px\"></div><p>x</p>", "", 10);
        assert_eq!(l[2], "x", "{l:?}");
    }

    #[test]
    fn width_em_px_define_a_caixa() {
        let saida = rodar_com(
            "<div style=\"width:160px;background:#222\">a</div>",
            "",
            80,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(com_fundo(&saida.linhas, Rgb(34, 34, 34)), 20);
    }

    #[test]
    fn min_width_e_piso_da_caixa() {
        let saida = rodar_com(
            "<div style=\"width:80px;min-width:320px;background:#222\">a</div>",
            "",
            80,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(com_fundo(&saida.linhas, Rgb(34, 34, 34)), 40);
    }

    #[test]
    fn bloco_vazio_com_fundo_e_altura_vira_barra() {
        let saida = rodar_com(
            "<div style=\"background:#222;height:8px\"></div>",
            "",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(saida.linhas.len(), 1);
        assert_eq!(com_fundo(&saida.linhas, Rgb(34, 34, 34)), 10);
    }

    #[test]
    fn barra_de_progresso_pinta_86_por_cento() {
        let html = "<div style=\"background:#333;height:8px\"><div style=\"width:86%;\
            height:8px;background:#3fb950\"></div></div>";
        let saida = rodar_com(html, "", 50, &Limits::default(), &Unlimited).unwrap();
        assert_eq!(saida.linhas.len(), 1);
        let verde = com_fundo(&saida.linhas, Rgb(63, 185, 80));
        assert!((42..=44).contains(&verde), "verde = {verde}");
        assert_eq!(com_fundo(&saida.linhas, Rgb(51, 51, 51)), 50 - verde);
    }

    #[test]
    fn barra_com_filho_de_altura_percentual() {
        let html = "<div style=\"background:#0b1220;height:10px\"><div style=\"width:64%;\
            height:100%;background:#eab308\"></div></div>";
        let saida = rodar_com(html, "", 50, &Limits::default(), &Unlimited).unwrap();
        assert_eq!(saida.linhas.len(), 1);
        let amarelo = com_fundo(&saida.linhas, Rgb(234, 179, 8));
        assert!((31..=33).contains(&amarelo), "amarelo = {amarelo}");
    }

    #[test]
    fn titulos_registram_linha() {
        let saida = rodar_com(
            "<p>a</p><h2>Seção</h2>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(saida.titulos, vec![(2, 2, "Seção".to_string())]);
    }

    /// Texto de todas as linhas, para mensagens de falha.
    fn textos(linhas: &[Line]) -> Vec<String> {
        linhas.iter().map(|l| texto(l)).collect()
    }

    const VERMELHO: Rgb = Rgb(255, 0, 0);

    #[test]
    fn caixa_com_quatro_bordas() {
        let saida = rodar_com(
            "<div style=\"border:1px solid #f00;background:#222\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let l = &saida.linhas;
        assert_eq!(l.len(), 3, "{:?}", textos(l));
        let topo = texto(&l[0]);
        let base = texto(&l[2]);
        assert!(topo.starts_with('┌') && topo.ends_with('┐'), "{topo:?}");
        assert!(base.starts_with('└') && base.ends_with('┘'), "{base:?}");
        let meio = texto(&l[1]);
        assert!(meio.starts_with("│ oi") && meio.ends_with('│'), "{meio:?}");
        assert!(l.iter().all(|x| line_width(x) == 20), "{:?}", textos(l));
        assert!(faces(l, '┌').iter().all(|f| f.fg == Some(VERMELHO)));
        assert!(faces(l, '│').iter().all(|f| f.fg == Some(VERMELHO)));
        assert!(faces(l, '│').iter().all(|f| f.bg == Some(Rgb(34, 34, 34))));
    }

    #[test]
    fn caixa_arredondada() {
        let saida = rodar_com(
            "<div style=\"border:1px solid #f00;border-radius:8px\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let l = &saida.linhas;
        assert!(texto(&l[0]).starts_with('╭') && texto(&l[0]).ends_with('╮'), "{:?}", textos(l));
        assert!(texto(&l[2]).starts_with('╰') && texto(&l[2]).ends_with('╯'), "{:?}", textos(l));
    }

    #[test]
    fn raio_pequeno_mantem_cantos_retos() {
        let saida = rodar_com(
            "<div style=\"border:1px solid #f00;border-radius:2px\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert!(texto(&saida.linhas[0]).starts_with('┌'), "{:?}", textos(&saida.linhas));
    }

    #[test]
    fn so_borda_esquerda_continua_barra() {
        let saida = rodar_com(
            "<div style=\"border-left:3px solid #f00\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(texto(&saida.linhas[0]), "▌ oi");
        assert!(faces(&saida.linhas, '▌').iter().all(|f| f.fg == Some(VERMELHO)));
        assert!(faces(&saida.linhas, '│').is_empty(), "{:?}", textos(&saida.linhas));
    }

    #[test]
    fn so_borda_direita_vira_barra_direita() {
        let saida = rodar_com(
            "<div style=\"border-right:3px solid #f00\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let l0 = &saida.linhas[0];
        assert_eq!(l0.len(), 20);
        assert_eq!(l0[19].ch, '▐', "{:?}", textos(&saida.linhas));
        assert_eq!(l0[19].face.fg, Some(VERMELHO));
        assert!(faces(&saida.linhas, '│').is_empty());
    }

    #[test]
    fn esquerda_e_direita_sem_topo() {
        let saida = rodar_com(
            "<div style=\"border-left:1px solid #f00;border-right:1px solid #f00\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let l = &saida.linhas;
        assert_eq!(l.len(), 1, "{:?}", textos(l));
        let linha = texto(&l[0]);
        assert!(linha.starts_with("│ oi") && linha.ends_with('│'), "{linha:?}");
        assert!(faces(l, '┌').is_empty() && faces(l, '─').is_empty());
    }

    #[test]
    fn so_topo_vira_regua() {
        let saida = rodar_com(
            "<div style=\"border-top:1px solid #f00\">oi</div>",
            "",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(textos(&saida.linhas), vec!["─".repeat(10), "oi".to_string()]);
        assert!(faces(&saida.linhas, '─').iter().all(|f| f.fg == Some(VERMELHO)));
    }

    #[test]
    fn borda_inferior_isolada_vira_regua_abaixo() {
        let saida = rodar_com(
            "<div style=\"border-bottom:1px solid #f00\">oi</div>",
            "",
            10,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(textos(&saida.linhas), vec!["oi".to_string(), "─".repeat(10)]);
    }

    #[test]
    fn caixa_sem_fundo_tambem_desenha() {
        let saida = rodar_com(
            "<div style=\"border:1px solid #f00\">oi</div>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        assert_eq!(saida.linhas.len(), 3, "{:?}", textos(&saida.linhas));
        assert!(texto(&saida.linhas[0]).starts_with('┌'));
        assert!(texto(&saida.linhas[2]).starts_with('└'));
    }

    #[test]
    fn tabela_nao_ganha_caixa_extra() {
        let saida = rodar_com(
            "<table style=\"border:1px solid #f00\"><tr><td>a</td><td>b</td></tr></table>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let todo = textos(&saida.linhas).join("\n");
        for canto in ['┌', '┐', '└', '┘', '╭', '╮', '╰', '╯'] {
            assert!(!todo.contains(canto), "{todo:?}");
        }
    }

    #[test]
    fn li_com_borda_nao_ganha_caixa() {
        let saida = rodar_com(
            "<ul><li style=\"border:1px solid #f00\">a</li></ul>",
            "",
            20,
            &Limits::default(),
            &Unlimited,
        )
        .unwrap();
        let todo = textos(&saida.linhas).join("\n");
        assert!(!todo.contains('┌') && !todo.contains('└'), "{todo:?}");
    }

    #[test]
    fn caixa_estreita_nao_estoura_largura() {
        for w in [4usize, 6, 10, 20] {
            let saida = rodar_com(
                "<div style=\"border:1px solid #f00;padding:8px\">oi</div>",
                "",
                w,
                &Limits::default(),
                &Unlimited,
            )
            .unwrap();
            for l in &saida.linhas {
                assert_eq!(line_width(l), w, "largura {w}: {:?}", textos(&saida.linhas));
            }
            let todo = textos(&saida.linhas).join("\n");
            assert!(todo.contains("oi"), "largura {w}: {todo:?}");
            if w == 4 {
                // Sem espaço para as bordas laterais: só as réguas, sem cantos.
                assert_eq!(texto(&saida.linhas[0]), "────", "{todo:?}");
            }
        }
    }

    #[test]
    fn alturas_iguais_com_caixas_em_grid() {
        let html = "<div style=\"display:grid;grid-template-columns:repeat(3,1fr)\">\
            <div style=\"border:1px solid #f00\">a</div>\
            <div style=\"border:1px solid #f00\">b<br>c<br>d</div>\
            <div style=\"border:1px solid #f00\">e<br>f</div></div>";
        let saida = rodar_com(html, "", 60, &Limits::default(), &Unlimited).unwrap();
        let l = &saida.linhas;
        let com_base: Vec<usize> = l
            .iter()
            .enumerate()
            .filter(|(_, x)| x.iter().any(|c| c.ch == '└'))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(com_base.len(), 1, "{:?}", textos(l));
        let base = &l[com_base[0]];
        assert_eq!(base.iter().filter(|c| c.ch == '└').count(), 3, "{:?}", textos(l));
        assert_eq!(l.len(), 5, "{:?}", textos(l));
    }

    #[test]
    fn caixa_aninhada() {
        let html = "<div style=\"border:1px solid #f00\">\
            <div style=\"border:1px solid #0f0\">x</div></div>";
        let saida = rodar_com(html, "", 20, &Limits::default(), &Unlimited).unwrap();
        let l = &saida.linhas;
        assert_eq!(l.len(), 5, "{:?}", textos(l));
        assert_eq!(l[1][2].ch, '┌', "{:?}", textos(l));
        assert_eq!(l[2][2].ch, '│', "{:?}", textos(l));
        assert_eq!(l[3][2].ch, '└', "{:?}", textos(l));
        assert!(texto(&l[4]).starts_with('└'), "{:?}", textos(l));
    }
}
