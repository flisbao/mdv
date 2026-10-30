//! Texto em células: espaços, transformação de caixa, quebra de linha e alinhamento.
//! Porte de `push_text`, `wrap`, `pad_line` e `truncate` do protótipo (`spike.rs.txt`),
//! com os modos de `white-space` e `text-transform`. Sem I/O.

use crate::model::{line_width, Cell, Face, Line, Rgb};
use crate::style::{Align, Transform, WhiteSpace};
use unicode_width::UnicodeWidthChar;

/// Espaços que um `\t` vira em texto pré-formatado.
const TAB: usize = 4;

fn largura_char(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

fn celula(ch: char, face: Face) -> Cell {
    Cell { ch, face }
}

fn espacos(n: usize, bg: Option<Rgb>) -> Line {
    vec![celula(' ', Face { bg, ..Face::default() }); n]
}

/// Caracteres de largura zero que não aparecem no texto (juntores e marca de ordem de bytes).
fn invisivel(ch: char) -> bool {
    matches!(ch, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Acrescenta `ch` aplicando a transformação de caixa. Em `Capitalize`, só vira maiúscula
/// no início de palavra (começo da saída ou depois de espaço).
fn emite(out: &mut Vec<Cell>, ch: char, face: Face, transform: Transform) {
    let inicio = out.last().is_none_or(|c| c.ch.is_whitespace());
    match transform {
        Transform::None => out.push(celula(ch, face)),
        Transform::Upper => out.extend(ch.to_uppercase().map(|c| celula(c, face))),
        Transform::Lower => out.extend(ch.to_lowercase().map(|c| celula(c, face))),
        Transform::Capitalize if inicio => {
            out.extend(ch.to_uppercase().map(|c| celula(c, face)));
        }
        Transform::Capitalize => out.push(celula(ch, face)),
    }
}

/// Acrescenta `text` às células, tratando espaços, controles e a transformação de caixa.
/// Em `Normal` e `NoWrap` cada espaço em branco vira `' '` (menos o NBSP, que fica como está);
/// em `Pre` e `PreWrap` espaços e `'\n'` são mantidos e `'\t'` vira quatro espaços.
/// Controles e caracteres de largura zero somem.
pub fn push_text(
    out: &mut Vec<Cell>,
    text: &str,
    face: Face,
    white: WhiteSpace,
    transform: Transform,
) {
    let preserva = matches!(white, WhiteSpace::Pre | WhiteSpace::PreWrap);
    for ch in text.chars() {
        if invisivel(ch) {
            continue;
        }
        if preserva {
            match ch {
                '\n' => out.push(celula('\n', face)),
                '\t' => out.extend((0..TAB).map(|_| celula(' ', face))),
                c if c.is_control() => {}
                c => emite(out, c, face, transform),
            }
        } else if ch == '\u{a0}' {
            out.push(celula(ch, face));
        } else if ch.is_whitespace() {
            out.push(celula(' ', face));
        } else if !ch.is_control() {
            emite(out, ch, face, transform);
        }
    }
}

/// Fecha a linha corrente e a põe em `linhas`.
fn fecha(linhas: &mut Vec<Line>, cur: &mut Line, cur_w: &mut usize) {
    linhas.push(std::mem::take(cur));
    *cur_w = 0;
}

/// Quebra um trecho sem `'\n'` com espaços normalizados (`Normal` e `NoWrap`).
/// Guloso por palavra; palavra mais larga que o limite é cortada por caractere.
fn quebra_normal(seg: &[Cell], limite: usize, linhas: &mut Vec<Line>) {
    let mut cur: Line = Vec::new();
    let mut cur_w = 0usize;
    // Espaço ainda não escrito; só existe com linha corrente não vazia (sem espaço inicial).
    let mut pendente: Option<Face> = None;
    let mut i = 0;
    while let Some(c) = seg.get(i) {
        if c.ch == ' ' {
            if !cur.is_empty() && pendente.is_none() {
                pendente = Some(c.face);
            }
            i += 1;
            continue;
        }
        let fim = seg
            .get(i..)
            .and_then(|r| r.iter().position(|x| x.ch == ' '))
            .map_or(seg.len(), |p| i + p);
        let palavra = seg.get(i..fim).unwrap_or(&[]);
        let ww = line_width(palavra);
        let sep = usize::from(pendente.is_some());
        if !cur.is_empty() && cur_w.saturating_add(sep).saturating_add(ww) > limite {
            fecha(linhas, &mut cur, &mut cur_w);
            pendente = None;
        }
        if let Some(f) = pendente.take() {
            cur.push(celula(' ', f));
            cur_w += 1;
        }
        for x in palavra {
            let w = largura_char(x.ch);
            if !cur.is_empty() && cur_w.saturating_add(w) > limite {
                fecha(linhas, &mut cur, &mut cur_w);
            }
            cur.push(*x);
            cur_w += w;
        }
        i = fim;
    }
    if !cur.is_empty() {
        linhas.push(cur);
    }
}

/// Quebra um trecho sem `'\n'` preservando espaços (`Pre` e `PreWrap`): corta por caractere.
fn quebra_pre(seg: &[Cell], limite: usize, linhas: &mut Vec<Line>) {
    let mut cur: Line = Vec::new();
    let mut cur_w = 0usize;
    for x in seg {
        let w = largura_char(x.ch);
        if !cur.is_empty() && cur_w.saturating_add(w) > limite {
            fecha(linhas, &mut cur, &mut cur_w);
        }
        cur.push(*x);
        cur_w += w;
    }
    if !cur.is_empty() {
        linhas.push(cur);
    }
}

/// Quebra as células na largura pedida (`width == 0` vale 1). Cada `'\n'` força quebra.
/// Em `NoWrap` não há quebra por largura (o chamador trunca). Um trecho vazio entre dois
/// `'\n'` vira linha em branco; no fim do texto não gera linha extra.
pub fn wrap(cells: &[Cell], width: usize, white: WhiteSpace) -> Vec<Line> {
    let largura = width.max(1);
    let limite = if white == WhiteSpace::NoWrap {
        usize::MAX
    } else {
        largura
    };
    let preserva = matches!(white, WhiteSpace::Pre | WhiteSpace::PreWrap);
    let segmentos: Vec<&[Cell]> = cells.split(|c| c.ch == '\n').collect();
    let total = segmentos.len();
    let mut linhas = Vec::new();
    for (k, seg) in segmentos.into_iter().enumerate() {
        let antes = linhas.len();
        if preserva {
            quebra_pre(seg, limite, &mut linhas);
        } else {
            quebra_normal(seg, limite, &mut linhas);
        }
        if linhas.len() == antes && k + 1 < total {
            linhas.push(Vec::new());
        }
    }
    linhas
}

/// Preenche a linha com espaços até `width` conforme o alinhamento (no centro, o resto ímpar
/// vai para a direita). Linha mais larga volta intacta.
pub fn align(line: Line, width: usize, align: Align, bg: Option<Rgb>) -> Line {
    let folga = width.saturating_sub(line_width(&line));
    let (esq, dir) = match align {
        Align::Left => (0, folga),
        Align::Center => (folga / 2, folga - folga / 2),
        Align::Right => (folga, 0),
    };
    let mut out = espacos(esq, bg);
    out.extend(line);
    out.extend(espacos(dir, bg));
    out
}

/// Corta a linha para caber em `width` colunas, terminando com `…` quando corta.
pub fn truncate(line: &[Cell], width: usize) -> Line {
    if line_width(line) <= width {
        return line.to_vec();
    }
    if width == 0 {
        return Vec::new();
    }
    let mut out: Line = Vec::new();
    let mut x = 0usize;
    for c in line {
        let w = largura_char(c.ch);
        if x + w + 1 > width {
            break;
        }
        out.push(*c);
        x += w;
    }
    let face = out
        .last()
        .or(line.first())
        .map(|c| c.face)
        .unwrap_or_default();
    out.push(celula('…', face));
    out
}

/// Linha vazia de `width` colunas com o fundo dado.
pub fn blank(width: usize, bg: Option<Rgb>) -> Line {
    espacos(width, bg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::line_width;

    fn celulas(texto: &str, white: WhiteSpace, t: Transform) -> Vec<Cell> {
        let mut v = Vec::new();
        push_text(&mut v, texto, Face::default(), white, t);
        v
    }

    fn normal(texto: &str) -> Vec<Cell> {
        celulas(texto, WhiteSpace::Normal, Transform::None)
    }

    fn txt(l: &[Cell]) -> String {
        l.iter().map(|c| c.ch).collect()
    }

    fn txts(ls: &[Line]) -> Vec<String> {
        ls.iter().map(|l| txt(l)).collect()
    }

    #[test]
    fn colapsa_espacos() {
        let l = wrap(&normal("a   b\n c"), 80, WhiteSpace::Normal);
        assert_eq!(txts(&l), vec!["a b c"]);
    }

    #[test]
    fn nbsp_nao_quebra() {
        let l = wrap(&normal("aa\u{a0}bb cc"), 5, WhiteSpace::Normal);
        assert_eq!(txts(&l), vec!["aa\u{a0}bb", "cc"]);
    }

    #[test]
    fn quebra_gulosa() {
        let l = wrap(&normal("um dois tres quatro"), 9, WhiteSpace::Normal);
        assert_eq!(txts(&l), vec!["um dois", "tres", "quatro"]);
    }

    #[test]
    fn palavra_longa_e_cortada() {
        let l = wrap(&normal("abcdefghij"), 4, WhiteSpace::Normal);
        assert_eq!(txts(&l), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn largura_cjk_e_emoji() {
        let l = wrap(&normal("日本語"), 4, WhiteSpace::Normal);
        assert_eq!(txts(&l), vec!["日本", "語"]);
        let e = wrap(&normal("😀😀😀"), 4, WhiteSpace::Normal);
        assert_eq!(txts(&e), vec!["😀😀", "😀"]);
        assert!(e.iter().all(|l| line_width(l) <= 4));
    }

    #[test]
    fn pre_preserva() {
        let c = celulas("a  b\n  c", WhiteSpace::Pre, Transform::None);
        let l = wrap(&c, 10, WhiteSpace::Pre);
        assert_eq!(txts(&l), vec!["a  b", "  c"]);
    }

    #[test]
    fn pre_tab_vira_quatro_espacos() {
        let c = celulas("a\tb", WhiteSpace::Pre, Transform::None);
        assert_eq!(txt(&c), "a    b");
    }

    #[test]
    fn quebra_forcada() {
        let c = celulas("ab\ncd", WhiteSpace::Pre, Transform::None);
        let l = wrap(&c, 10, WhiteSpace::Pre);
        assert_eq!(txts(&l), vec!["ab", "cd"]);
        let vazia = wrap(&celulas("a\n\nb", WhiteSpace::Pre, Transform::None), 10, WhiteSpace::Pre);
        assert_eq!(txts(&vazia), vec!["a", "", "b"]);
    }

    #[test]
    fn nowrap_nao_quebra() {
        let l = wrap(&normal("um dois tres quatro"), 4, WhiteSpace::NoWrap);
        assert_eq!(txts(&l), vec!["um dois tres quatro"]);
    }

    #[test]
    fn align_centro() {
        let bg = Some(Rgb(1, 2, 3));
        let l = align(normal("ab"), 5, Align::Center, bg);
        assert_eq!(txt(&l), " ab  ");
        // Só o preenchimento recebe o fundo; o conteúdo mantém o seu.
        assert_eq!(l.first().map(|c| c.face.bg), Some(bg));
        assert_eq!(l.last().map(|c| c.face.bg), Some(bg));
        assert_eq!(l.get(1).map(|c| c.face.bg), Some(None));
        let d = align(normal("ab"), 5, Align::Right, None);
        assert_eq!(txt(&d), "   ab");
        let e = align(normal("ab"), 5, Align::Left, None);
        assert_eq!(txt(&e), "ab   ");
    }

    #[test]
    fn align_nao_corta() {
        let l = align(normal("abcdef"), 4, Align::Center, None);
        assert_eq!(txt(&l), "abcdef");
    }

    #[test]
    fn truncate_com_reticencias() {
        let c = normal("abcdefgh");
        assert_eq!(txt(&truncate(&c, 5)), "abcd…");
        assert_eq!(txt(&truncate(&c, 8)), "abcdefgh");
        assert!(line_width(&truncate(&c, 5)) <= 5);
    }

    #[test]
    fn transform() {
        let cap = celulas("olá mundo", WhiteSpace::Normal, Transform::Capitalize);
        assert_eq!(txt(&cap), "Olá Mundo");
        let up = celulas("olá mundo", WhiteSpace::Normal, Transform::Upper);
        assert_eq!(txt(&up), "OLÁ MUNDO");
        let low = celulas("OLÁ Mundo", WhiteSpace::Normal, Transform::Lower);
        assert_eq!(txt(&low), "olá mundo");
    }

    #[test]
    fn controle_descartado() {
        let c = normal("a\u{1b}[31mb\u{200b}\u{feff}\u{202e}");
        assert_eq!(txt(&c), "a[31mb");
        let r = celulas("a\r\nb", WhiteSpace::Pre, Transform::None);
        assert_eq!(txt(&r), "a\nb");
    }

    #[test]
    fn largura_zero_nao_panica() {
        let c = normal("abc de");
        let l = wrap(&c, 0, WhiteSpace::Normal);
        assert_eq!(txts(&l), vec!["a", "b", "c", "d", "e"]);
        assert!(wrap(&[], 0, WhiteSpace::Normal).is_empty());
        assert!(truncate(&c, 0).is_empty());
        assert!(blank(0, None).is_empty());
        assert_eq!(align(Vec::new(), 0, Align::Center, None).len(), 0);
    }

    #[test]
    fn blank_tem_largura_e_fundo() {
        let bg = Some(Rgb(9, 9, 9));
        let l = blank(3, bg);
        assert_eq!(txt(&l), "   ");
        assert!(l.iter().all(|c| c.face.bg == bg));
    }
}
