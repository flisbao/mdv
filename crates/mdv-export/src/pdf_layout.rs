//! Layout do PDF: quebra de linha, paginação, listas, tabelas e imagens, produzindo as posições
//! de desenho (`Op`) de cada página. A serialização fica em `pdf`.

use super::model::{Block, Inline};
use super::pdf_font::{encode, text_width, width, Face};
use super::Budget;
use image::DynamicImage;
use pulldown_cmark::Alignment;
use std::collections::BTreeSet;
use std::sync::Arc;

pub(crate) const PAGE_W: f32 = 595.0;
pub(crate) const PAGE_H: f32 = 842.0;
pub(crate) const MARGIN: f32 = 56.0;
/// Teto de páginas.
pub const MAX_PAGES: usize = 2000;
const BODY_SIZE: f32 = 11.0;
const BODY_LEADING: f32 = 14.5;
const CODE_SIZE: f32 = 9.0;
const CODE_LEADING: f32 = 11.5;
const TABLE_SIZE: f32 = 10.0;
const TABLE_LEADING: f32 = 13.0;
const TABLE_PAD: f32 = 4.0;
const MAX_CELL_LINES: usize = 40;
const LIST_INDENT: f32 = 18.0;
const QUOTE_INDENT: f32 = 14.0;
const MAX_IMAGE_H: f32 = 300.0;
/// Pontos por pixel (96 dpi → 72 dpi).
const PT_PER_PX: f32 = 0.75;
/// Quantos caracteres perdidos aparecem no aviso.
const MAX_LOST_SHOWN: usize = 8;

pub(crate) type Rgb = (f32, f32, f32);
const BLACK: Rgb = (0.0, 0.0, 0.0);
const BLUE: Rgb = (0.0, 0.2, 0.8);

/// Instrução de desenho; `top` e `y` medem a distância desde o topo da página, em pontos.
pub(crate) enum Op {
    Text { x: f32, y: f32, face: Face, size: f32, color: Rgb, bytes: Vec<u8> },
    Rect { x: f32, top: f32, w: f32, h: f32, gray: f32 },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, gray: f32, width: f32 },
    Image { index: usize, x: f32, top: f32, w: f32, h: f32 },
}

#[derive(Clone)]
struct Word {
    bytes: Vec<u8>,
    face: Face,
    size: f32,
    color: Rgb,
    underline: bool,
    strike: bool,
}

#[derive(Default)]
struct Line {
    /// Palavra e posição horizontal relativa ao início da linha.
    words: Vec<(f32, Word)>,
    width: f32,
}

struct Marker {
    bytes: Vec<u8>,
    x: f32,
}

pub(crate) struct Layout<'a> {
    budget: &'a dyn Budget,
    pub(crate) pages: Vec<Vec<Op>>,
    /// Distância do topo da página até o próximo elemento.
    top: f32,
    pub(crate) images: Vec<Arc<DynamicImage>>,
    marker: Option<Marker>,
    /// Posições `x` das barras de citação abertas.
    bars: Vec<f32>,
    lost: BTreeSet<char>,
    max_pages: usize,
}

impl<'a> Layout<'a> {
    pub(crate) fn new(budget: &'a dyn Budget) -> Self {
        Layout {
            budget,
            pages: vec![Vec::new()],
            top: MARGIN,
            images: Vec::new(),
            marker: None,
            bars: Vec::new(),
            lost: BTreeSet::new(),
            max_pages: MAX_PAGES,
        }
    }

    pub(crate) fn warnings(&self) -> Vec<String> {
        if self.lost.is_empty() {
            return Vec::new();
        }
        let shown: String = self.lost.iter().take(MAX_LOST_SHOWN).map(|c| c.to_string()).collect::<Vec<_>>().join(" ");
        vec![format!(
            "{} caractere(s) fora do Latin-1 viraram '?' no PDF (fontes padrão do formato): {shown}",
            self.lost.len()
        )]
    }

    fn page(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().expect("sempre há uma página")
    }

    fn new_page(&mut self) -> Result<(), String> {
        if self.pages.len() >= self.max_pages {
            return Err(format!("documento longo demais para PDF (limite de {} páginas)", self.max_pages));
        }
        self.pages.push(Vec::new());
        self.top = MARGIN;
        Ok(())
    }

    /// Garante `h` pontos livres na página; senão abre outra (a não ser que já esteja no topo).
    fn ensure(&mut self, h: f32) -> Result<(), String> {
        if self.top + h > PAGE_H - MARGIN && self.top > MARGIN + 0.5 {
            self.new_page()?;
        }
        Ok(())
    }

    fn byte_of(&mut self, c: char) -> u8 {
        match encode(c) {
            Some(b) => b,
            None => {
                self.lost.insert(c);
                b'?'
            }
        }
    }

    /// Quebra os trechos em linhas de no máximo `avail` pontos.
    fn break_lines(&mut self, inlines: &[Inline], avail: f32, size: f32, bold: bool) -> Vec<Line> {
        let mut lines: Vec<Line> = Vec::new();
        let mut cur = Line::default();
        let mut pending_space = false;
        for item in inlines {
            match item {
                Inline::Break => {
                    lines.push(std::mem::take(&mut cur));
                    pending_space = false;
                }
                Inline::Run(r) => {
                    let proto = Word {
                        bytes: Vec::new(),
                        face: Face::of(r.bold || bold, r.italic, r.code),
                        size,
                        color: if r.link.is_some() { BLUE } else { BLACK },
                        underline: r.link.is_some(),
                        strike: r.strike,
                    };
                    let mut bytes: Vec<u8> = Vec::new();
                    for ch in r.text.chars() {
                        if ch.is_whitespace() {
                            if !bytes.is_empty() {
                                place(&proto, std::mem::take(&mut bytes), pending_space, avail, &mut cur, &mut lines);
                            }
                            pending_space = true;
                        } else {
                            let b = self.byte_of(ch);
                            bytes.push(b);
                        }
                    }
                    if !bytes.is_empty() {
                        place(&proto, bytes, pending_space, avail, &mut cur, &mut lines);
                        pending_space = false;
                    }
                }
            }
        }
        if !cur.words.is_empty() || lines.is_empty() {
            lines.push(cur);
        }
        lines
    }

    /// Desenha as palavras de uma linha com a linha de base em `baseline`.
    fn push_words(&mut self, line: &Line, x: f32, baseline: f32) {
        for (wx, w) in &line.words {
            let ww = text_width(w.face, &w.bytes, w.size);
            let ops = self.page();
            ops.push(Op::Text { x: x + wx, y: baseline, face: w.face, size: w.size, color: w.color, bytes: w.bytes.clone() });
            if w.underline {
                ops.push(Op::Line { x1: x + wx, y1: baseline + 1.5, x2: x + wx + ww, y2: baseline + 1.5, gray: 0.0, width: 0.5 });
            }
            if w.strike {
                let y = baseline - w.size * 0.3;
                ops.push(Op::Line { x1: x + wx, y1: y, x2: x + wx + ww, y2: y, gray: 0.0, width: 0.5 });
            }
        }
    }

    /// Escreve uma linha de texto na posição atual e avança `leading`.
    fn put_line(&mut self, line: &Line, x: f32, leading: f32, size: f32) -> Result<(), String> {
        self.ensure(leading)?;
        let top = self.top;
        let baseline = top + leading * 0.75;
        for bx in self.bars.clone() {
            self.page().push(Op::Rect { x: bx, top, w: 2.0, h: leading, gray: 0.6 });
        }
        if let Some(m) = self.marker.take() {
            self.page().push(Op::Text { x: m.x, y: baseline, face: Face::Regular, size, color: BLACK, bytes: m.bytes });
        }
        self.push_words(line, x, baseline);
        self.top += leading;
        Ok(())
    }

    fn para(&mut self, inlines: &[Inline], x: f32, w: f32, size: f32, leading: f32, bold: bool) -> Result<(), String> {
        for line in self.break_lines(inlines, w, size, bold) {
            self.put_line(&line, x, leading, size)?;
        }
        Ok(())
    }

    pub(crate) fn block(&mut self, b: &Block, x: f32, w: f32) -> Result<(), String> {
        self.budget.check()?;
        match b {
            Block::Heading { level, text } => {
                let size = [22.0, 18.0, 15.0, 13.0, 11.5, 11.0][usize::from((*level).clamp(1, 6)) - 1];
                if self.top > MARGIN + 0.5 {
                    self.top += 10.0;
                }
                self.ensure(size * 1.25 * 2.0)?;
                self.para(text, x, w, size, size * 1.25, true)?;
                self.top += 4.0;
            }
            Block::Paragraph(text) => {
                self.para(text, x, w, BODY_SIZE, BODY_LEADING, false)?;
                self.top += 6.0;
            }
            Block::List { ordered, start, items } => {
                for (i, item) in items.iter().enumerate() {
                    let marker = match item.task {
                        Some(true) => "[x]".to_string(),
                        Some(false) => "[ ]".to_string(),
                        None if *ordered => format!("{}.", start + i as u64),
                        None => "\u{2022}".to_string(),
                    };
                    let bytes: Vec<u8> = marker.chars().map(|c| self.byte_of(c)).collect();
                    self.marker = Some(Marker { bytes, x });
                    for inner in &item.blocks {
                        self.block(inner, x + LIST_INDENT, w - LIST_INDENT)?;
                    }
                    if self.marker.is_some() {
                        self.put_line(&Line::default(), x, BODY_LEADING, BODY_SIZE)?;
                    }
                }
                self.top += 2.0;
            }
            Block::Quote { title, blocks } => {
                self.bars.push(x + 3.0);
                if let Some(t) = title {
                    let inl = vec![Inline::Run(super::model::Run::bold(t.clone()))];
                    self.para(&inl, x + QUOTE_INDENT, w - QUOTE_INDENT, BODY_SIZE, BODY_LEADING, true)?;
                }
                for inner in blocks {
                    self.block(inner, x + QUOTE_INDENT, w - QUOTE_INDENT)?;
                }
                self.bars.pop();
            }
            Block::Code { text, .. } => self.code(text, x, w)?,
            Block::Table { aligns, header, rows } => self.table(aligns, header, rows, x, w)?,
            Block::Rule => {
                self.ensure(12.0)?;
                let y = self.top + 6.0;
                self.page().push(Op::Line { x1: x, y1: y, x2: x + w, y2: y, gray: 0.6, width: 0.75 });
                self.top += 12.0;
            }
            Block::Picture { image, .. } => self.picture(image, x, w)?,
            Block::Image { alt, .. } => {
                let inl = vec![Inline::Run(super::model::Run::plain(format!("[imagem: {alt}]")))];
                self.para(&inl, x, w, BODY_SIZE, BODY_LEADING, false)?;
                self.top += 6.0;
            }
        }
        Ok(())
    }

    fn code(&mut self, text: &str, x: f32, w: f32) -> Result<(), String> {
        let per_line = (((w - 10.0) / (0.6 * CODE_SIZE)).floor() as usize).max(1);
        for raw in text.split('\n') {
            let bytes: Vec<u8> = raw.replace('\t', "    ").chars().map(|c| self.byte_of(c)).collect();
            let chunks: Vec<&[u8]> = if bytes.is_empty() { vec![&[][..]] } else { bytes.chunks(per_line).collect() };
            for chunk in chunks {
                self.ensure(CODE_LEADING)?;
                let top = self.top;
                let ops = self.page();
                ops.push(Op::Rect { x, top, w, h: CODE_LEADING, gray: 0.95 });
                if !chunk.is_empty() {
                    ops.push(Op::Text { x: x + 5.0, y: top + CODE_LEADING * 0.75, face: Face::Mono, size: CODE_SIZE, color: BLACK, bytes: chunk.to_vec() });
                }
                self.top += CODE_LEADING;
            }
        }
        self.top += 6.0;
        Ok(())
    }

    fn table(&mut self, aligns: &[Alignment], header: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], x: f32, w: f32) -> Result<(), String> {
        let cols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0)).max(1);
        let col_w = w / cols as f32;
        let mut all: Vec<(&[Vec<Inline>], bool)> = Vec::new();
        if !header.is_empty() {
            all.push((header, true));
        }
        all.extend(rows.iter().map(|r| (r.as_slice(), false)));
        for (cells, head) in all {
                self.budget.check()?;
            let laid: Vec<Vec<Line>> = (0..cols)
                .map(|c| match cells.get(c) {
                    Some(cell) => self.break_lines(cell, col_w - 2.0 * TABLE_PAD, TABLE_SIZE, head),
                    None => Vec::new(),
                })
                .collect();
            let n = laid.iter().map(Vec::len).max().unwrap_or(1).clamp(1, MAX_CELL_LINES);
            let h = n as f32 * TABLE_LEADING + 2.0 * TABLE_PAD;
            self.ensure(h)?;
            let top = self.top;
            if head {
                self.page().push(Op::Rect { x, top, w, h, gray: 0.9 });
            }
            for (c, lines) in laid.iter().enumerate() {
                let cx = x + c as f32 * col_w;
                let ops = self.page();
                ops.push(Op::Line { x1: cx, y1: top, x2: cx + col_w, y2: top, gray: 0.5, width: 0.5 });
                ops.push(Op::Line { x1: cx, y1: top + h, x2: cx + col_w, y2: top + h, gray: 0.5, width: 0.5 });
                ops.push(Op::Line { x1: cx, y1: top, x2: cx, y2: top + h, gray: 0.5, width: 0.5 });
                ops.push(Op::Line { x1: cx + col_w, y1: top, x2: cx + col_w, y2: top + h, gray: 0.5, width: 0.5 });
                for (i, line) in lines.iter().take(n).enumerate() {
                    let room = col_w - 2.0 * TABLE_PAD - line.width;
                    let shift = match aligns.get(c).copied().unwrap_or(Alignment::None) {
                        Alignment::Center => room / 2.0,
                        Alignment::Right => room,
                        _ => 0.0,
                    };
                    let baseline = top + TABLE_PAD + i as f32 * TABLE_LEADING + TABLE_LEADING * 0.75;
                    self.push_words(line, cx + TABLE_PAD + shift.max(0.0), baseline);
                }
            }
            self.top += h;
        }
        self.top += 8.0;
        Ok(())
    }

    fn picture(&mut self, image: &Arc<DynamicImage>, x: f32, w: f32) -> Result<(), String> {
        let (iw, ih) = (image.width() as f32 * PT_PER_PX, image.height() as f32 * PT_PER_PX);
        let scale = (w / iw).min(MAX_IMAGE_H / ih).min(1.0);
        let (pw, ph) = ((iw * scale).max(1.0), (ih * scale).max(1.0));
        self.ensure(ph + 8.0)?;
        let index = self.images.len();
        self.images.push(Arc::clone(image));
        let top = self.top;
        self.page().push(Op::Image { index, x, top, w: pw, h: ph });
        self.top += ph + 8.0;
        Ok(())
    }
}

/// Põe `bytes` como próxima palavra de `cur`, abrindo linha nova quando não cabe. Palavra mais
/// larga que a linha (um endereço longo) é partida em pedaços que cabem.
fn place(proto: &Word, bytes: Vec<u8>, space: bool, avail: f32, cur: &mut Line, lines: &mut Vec<Line>) {
    let mut rest = bytes;
    let mut space = space;
    loop {
        let w = text_width(proto.face, &rest, proto.size);
        let space_w = if space && !cur.words.is_empty() { text_width(proto.face, b" ", proto.size) } else { 0.0 };
        if cur.width + space_w + w <= avail {
            let x = cur.width + space_w;
            cur.words.push((x, Word { bytes: rest, ..proto.clone() }));
            cur.width = x + w;
            return;
        }
        if !cur.words.is_empty() {
            lines.push(std::mem::take(cur));
            space = false;
            continue;
        }
        let mut n = 0;
        let mut used = 0.0;
        for &b in &rest {
            let bw = f32::from(width(proto.face, b) as u16) * proto.size / 1000.0;
            if used + bw > avail && n > 0 {
                break;
            }
            used += bw;
            n += 1;
        }
        let tail = rest.split_off(n);
        cur.width = text_width(proto.face, &rest, proto.size);
        cur.words.push((0.0, Word { bytes: rest, ..proto.clone() }));
        lines.push(std::mem::take(cur));
        if tail.is_empty() {
            return;
        }
        rest = tail;
        space = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    #[test]
    fn teto_de_paginas_e_respeitado() {
        let doc = parse(&"linha\n\n".repeat(200), &crate::Unlimited).unwrap();
        let mut l = Layout::new(&crate::Unlimited);
        l.max_pages = 2;
        let err = doc.blocks.iter().try_for_each(|b| l.block(b, MARGIN, PAGE_W - 2.0 * MARGIN)).unwrap_err();
        assert!(err.contains("limite de 2 páginas"), "{err}");
    }
}
