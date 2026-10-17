//! Quebra de linha e janelas por largura de coluna.

use super::*;

pub(super) fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Janela de `avail` colunas que começa na coluna `skip` de `pieces`. Marca com `‹`/`›` as
/// bordas onde há conteúdo cortado. Caractere largo que cruza a borda sai da janela (a linha
/// fica uma coluna mais curta; quem desenha completa com espaços).
pub(super) fn window_by_width(pieces: &[Styled], skip: usize, avail: usize) -> Vec<Styled> {
    let mut cells: Vec<(char, Style)> = Vec::new();
    let (mut col, mut used) = (0usize, 0usize);
    let (mut left, mut right) = (false, false);
    for (text, style) in pieces {
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            let start = col;
            col += cw;
            if right {
                continue;
            }
            if start < skip {
                left |= cw > 0;
                continue;
            }
            if used + cw > avail {
                right = true;
                continue;
            }
            cells.push((ch, *style));
            used += cw;
        }
    }
    let edge = Style::default().fg(DIM).bg(CODE_BG);
    if left {
        if let Some(first) = cells.first_mut() {
            *first = ('‹', edge);
        }
    }
    if right {
        if let Some(last) = cells.last_mut() {
            *last = ('›', edge);
        }
    }
    let mut out: Vec<Styled> = Vec::new();
    for (ch, st) in cells {
        match out.last_mut() {
            Some((s, last)) if *last == st => s.push(ch),
            _ => out.push((ch.to_string(), st)),
        }
    }
    out
}

/// Quebra em pedaços de no máximo `avail` colunas, caractere a caractere.
pub(super) fn split_by_width(pieces: &[Styled], avail: usize) -> Vec<Vec<Styled>> {
    let avail = avail.max(1);
    let mut done: Vec<Vec<Styled>> = Vec::new();
    let mut line: Vec<Styled> = Vec::new();
    let mut w = 0;
    for (text, style) in pieces {
        for ch in text.chars() {
            let mut cw = ch.width().unwrap_or(0);
            if w + cw > avail && w > 0 {
                done.push(std::mem::take(&mut line));
                w = 0;
            }
            // Caractere mais largo que a linha inteira (1 coluna): não cabe de jeito nenhum.
            let ch = if cw > avail {
                cw = REPLACEMENT.width().unwrap_or(1);
                REPLACEMENT
            } else {
                ch
            };
            match line.last_mut() {
                Some((s, st)) if *st == *style => s.push(ch),
                _ => line.push((ch.to_string(), *style)),
            }
            w += cw;
        }
    }
    done.push(line);
    done
}

pub(super) enum Unit {
    Space(Style),
    Word(Vec<Styled>),
    Break,
}

/// Espaços que separam palavras; os "sem quebra" (NBSP e similares) fazem parte da palavra.
pub(super) fn is_breaking_space(ch: char) -> bool {
    ch.is_whitespace() && !matches!(ch, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

pub(super) fn push_run(units: &mut Vec<Unit>, text: String, is_space: bool, style: Style) {
    if is_space {
        if !matches!(units.last(), Some(Unit::Space(_))) {
            units.push(Unit::Space(style));
        }
        return;
    }
    match units.last_mut() {
        Some(Unit::Word(p)) => p.push((text, style)),
        _ => units.push(Unit::Word(vec![(text, style)])),
    }
}

/// Monta linhas quebradas a partir de espaços e palavras.
pub(super) struct LineBuilder {
    pub(super) avail: usize,
    pub(super) done: Vec<Vec<Span<'static>>>,
    pub(super) line: Vec<Span<'static>>,
    /// Colunas já usadas na linha atual.
    pub(super) cur: usize,
    /// A linha atual termina num espaço que ainda pode ser descartado.
    pub(super) trailing: bool,
}

impl LineBuilder {
    pub(super) fn new(avail: usize) -> Self {
        LineBuilder { avail: avail.max(1), done: Vec::new(), line: Vec::new(), cur: 0, trailing: false }
    }

    pub(super) fn trim_trailing(&mut self) {
        if self.trailing {
            self.line.pop();
            self.cur = self.cur.saturating_sub(1);
            self.trailing = false;
        }
    }

    pub(super) fn new_line(&mut self) {
        self.done.push(std::mem::take(&mut self.line));
        self.cur = 0;
    }

    pub(super) fn break_line(&mut self) {
        self.trim_trailing();
        self.new_line();
    }

    pub(super) fn space(&mut self, style: Style) {
        if self.cur == 0 || self.cur + 1 > self.avail {
            return;
        }
        self.line.push(Span::styled(" ", style));
        self.cur += 1;
        self.trailing = true;
    }

    pub(super) fn word(&mut self, pieces: Vec<Styled>) {
        let ww: usize = pieces.iter().map(|(t, _)| t.width()).sum();
        if self.cur > 0 && self.cur + ww > self.avail {
            self.break_line();
        }
        self.trailing = false;
        if ww <= self.avail {
            self.line.extend(pieces.into_iter().map(|(t, st)| Span::styled(t, st)));
            self.cur += ww;
            return;
        }
        for (i, chunk) in split_by_width(&pieces, self.avail).into_iter().enumerate() {
            if i > 0 {
                self.new_line();
            }
            self.cur = chunk.iter().map(|(t, _)| t.width()).sum();
            self.line.extend(chunk.into_iter().map(|(t, st)| Span::styled(t, st)));
        }
    }

    pub(super) fn finish(mut self) -> Vec<Vec<Span<'static>>> {
        self.trim_trailing();
        self.done.push(self.line);
        self.done
    }
}

/// Quebra de linha por palavras preservando o estilo de cada trecho.
/// Trechos colados (ex.: `**negrito**,`) formam uma única "palavra" e não se separam.
pub(super) fn wrap(toks: &[Piece], avail: usize) -> Vec<Vec<Span<'static>>> {
    let mut units: Vec<Unit> = Vec::new();
    for piece in toks {
        let (text, style) = match piece {
            Piece::Text(text, style) => (text, style),
            Piece::Code(text, style) => {
                push_run(&mut units, text.clone(), false, *style);
                continue;
            }
            Piece::Break => {
                units.push(Unit::Break);
                continue;
            }
        };
        let mut buf = String::new();
        let mut buf_space = false;
        for ch in text.chars() {
            let sp = is_breaking_space(ch);
            if !buf.is_empty() && sp != buf_space {
                push_run(&mut units, std::mem::take(&mut buf), buf_space, *style);
            }
            buf_space = sp;
            buf.push(ch);
        }
        if !buf.is_empty() {
            push_run(&mut units, buf, buf_space, *style);
        }
    }

    let mut lines = LineBuilder::new(avail);
    for unit in units {
        match unit {
            Unit::Break => lines.break_line(),
            Unit::Space(st) => lines.space(st),
            Unit::Word(pieces) => lines.word(pieces),
        }
    }
    lines.finish()
}
