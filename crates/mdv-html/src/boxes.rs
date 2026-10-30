//! Desenho das bordas de bloco: caixa com cantos (`┌─┐`, `│`, `└─┘`, ou `╭╮╰╯` com
//! `border-radius`), barras laterais (`▌`/`▐`) e réguas (`─`) de topo e base. Cada linha sai com
//! exatamente a largura da caixa. Quem calcula a geometria (recuos, bordas que cabem) é `flow.rs`.

use crate::flow::ajusta;
use crate::inline;
use crate::model::{Face, Line, Rgb};
use crate::style::{Display, Style};
use crate::text::blank;

/// Raio mínimo, em px, para os cantos arredondados (`╭╮╰╯`).
const RAIO_MIN: f32 = 4.0;

/// Geometria de uma caixa: colunas de recuo e de borda de cada lado, e cores.
pub(crate) struct Caixa {
    pub avail: usize,
    pub lo: usize,
    pub ro: usize,
    pub pl: usize,
    pub pr: usize,
    pub paint: Option<Rgb>,
    /// Fundo da caixa (ou o do pai, quando a caixa não tem fundo próprio).
    pub bg: Option<Rgb>,
    /// Cor da borda esquerda; `None` quando não há borda ou ela não cabe na largura.
    pub esq: Option<Rgb>,
    /// Cor da borda direita; `None` quando não há borda ou ela não cabe na largura.
    pub dir: Option<Rgb>,
    /// Cor da borda de topo, quando há.
    pub sup: Option<Rgb>,
    /// Cor da borda de base, quando há.
    pub inf: Option<Rgb>,
    /// Bloco que desenha caixa (não tabela, item de lista, link nem controle).
    pub caixa: bool,
    /// Cantos arredondados (`╭╮╰╯`).
    pub redondo: bool,
}

impl Caixa {
    /// Caixa completa: há topo ou base, ou os dois lados. As laterais viram `│`, não `▌`/`▐`.
    fn cheia(&self) -> bool {
        self.caixa
            && (self.sup.is_some() || self.inf.is_some() || (self.esq.is_some() && self.dir.is_some()))
    }
}

/// Verdadeiro se o elemento desenha caixa: bloco (`Block`, `Flex`, `Grid`, `ListItem`) que não é
/// tabela, item de lista, link nem controle de formulário.
pub(crate) fn caixa_de_bloco(nome: &str, st: &Style) -> bool {
    !matches!(nome, "li" | "a" | "td" | "th" | "tr" | "table")
        && !inline::e_controle(nome)
        && matches!(
            st.display,
            Display::Block | Display::Flex | Display::Grid | Display::ListItem
        )
}

/// Monta uma linha de conteúdo com recuos, bordas laterais e fundo, `avail` colunas exatas.
pub(crate) fn monta(c: &Caixa, interno: Line) -> Line {
    let mut linha = blank(c.lo, c.paint);
    if let Some(cor) = c.esq {
        let glifo = if c.cheia() { '│' } else { '▌' };
        linha.push(inline::celula(glifo, Face { fg: Some(cor), bg: c.bg, ..Face::default() }));
        linha.push(inline::celula(' ', Face { bg: c.bg, ..Face::default() }));
    }
    linha.extend(blank(c.pl, c.bg));
    linha.extend(interno);
    linha.extend(blank(c.pr, c.bg));
    if let Some(cor) = c.dir {
        linha.push(inline::celula(' ', Face { bg: c.bg, ..Face::default() }));
        let glifo = if c.cheia() { '│' } else { '▐' };
        linha.push(inline::celula(glifo, Face { fg: Some(cor), bg: c.bg, ..Face::default() }));
    }
    linha.extend(blank(c.ro, c.paint));
    ajusta(linha, c.avail, c.paint)
}

/// Régua horizontal da largura da caixa, na cor da borda de topo (`sup`) ou de base. Numa caixa
/// com laterais, os cantos caem nas colunas das bordas laterais.
pub(crate) fn linha_borda(c: &Caixa, sup: bool) -> Line {
    let w = c.avail.saturating_sub(c.lo + c.ro);
    let cor = if sup { c.sup } else { c.inf };
    let traco = Face { fg: cor, bg: c.bg, ..Face::default() };
    let mut linha = blank(c.lo, c.paint);
    for i in 0..w {
        let ch = if c.caixa && i == 0 && c.esq.is_some() {
            canto(sup, false, c.redondo)
        } else if c.caixa && i + 1 == w && c.dir.is_some() {
            canto(sup, true, c.redondo)
        } else {
            '─'
        };
        linha.push(inline::celula(ch, traco));
    }
    linha.extend(blank(c.ro, c.paint));
    ajusta(linha, c.avail, c.paint)
}

/// Canto da caixa: topo ou base (`sup`), direita ou esquerda, reto ou arredondado.
fn canto(sup: bool, direita: bool, redondo: bool) -> char {
    match (sup, direita, redondo) {
        (true, false, false) => '┌',
        (true, true, false) => '┐',
        (false, false, false) => '└',
        (false, true, false) => '┘',
        (true, false, true) => '╭',
        (true, true, true) => '╮',
        (false, false, true) => '╰',
        (false, true, true) => '╯',
    }
}

/// Verdadeiro se o raio de `border-radius` basta para cantos arredondados.
pub(crate) fn redondo(st: &Style) -> bool {
    st.radius >= RAIO_MIN
}
