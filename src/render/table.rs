//! Tabelas: construtor, larguras de coluna e desenho das linhas.

use super::*;

/// Tabela em construção: linhas completas mais a linha em andamento.
pub(super) struct TableBuilder {
    pub(super) aligns: Vec<Alignment>,
    pub(super) rows: Vec<Vec<Vec<Piece>>>,
    pub(super) cur: Vec<Vec<Piece>>,
    pub(super) has_header: bool,
    pub(super) in_head: bool,
}

impl TableBuilder {
    pub(super) fn new(aligns: Vec<Alignment>) -> Self {
        TableBuilder { aligns, rows: Vec::new(), cur: Vec::new(), has_header: false, in_head: false }
    }

    pub(super) fn begin_head(&mut self) {
        self.in_head = true;
        self.cur.clear();
    }

    pub(super) fn begin_row(&mut self) {
        self.cur.clear();
    }

    pub(super) fn push_cell(&mut self, cell: Vec<Piece>) {
        self.cur.push(cell);
    }

    pub(super) fn end_row(&mut self) {
        let row = std::mem::take(&mut self.cur);
        self.rows.push(row);
    }

    pub(super) fn end_head(&mut self) {
        self.end_row();
        self.has_header = true;
        self.in_head = false;
    }
}

/// Desenha a tabela em `avail` colunas. A coluna 0 fica sempre; as colunas roláveis mostradas
/// começam em `first_col` (limitado para a última coluna caber no máximo). Colunas que não
/// cabem (cada uma precisa de `MIN_COL_WIDTH` mais a borda) ficam de fora e são contadas no aviso.
pub(super) fn layout_table(
    t: &TableData,
    first: &[Span<'static>],
    cont: &[Span<'static>],
    avail: usize,
    first_col: usize,
) -> TableView {
    let total = t.total_cols();
    let fit_cols = (avail.saturating_sub(1) / (CELL_OVERHEAD + MIN_COL_WIDTH)).max(1);
    let shown = total.min(fit_cols);
    let scrollable = shown.saturating_sub(1);
    let first_col = if total <= shown {
        usize::from(total > 1)
    } else {
        first_col.clamp(1, total - scrollable)
    };
    let idx: Vec<usize> = std::iter::once(0).chain(first_col..first_col + scrollable).collect();
    let aligns: Vec<Alignment> =
        idx.iter().map(|&c| t.aligns.get(c).copied().unwrap_or(Alignment::None)).collect();
    let widths = compute_column_widths(&natural_widths(&t.rows, &idx), avail);

    let mut drawn_rows: Vec<Vec<Line<'static>>> = Vec::with_capacity(t.rows.len());
    for row in &t.rows {
        if mem::exceeded() {
            break;
        }
        let cells: Vec<&[Piece]> =
            idx.iter().map(|&c| row.get(c).map_or(&[][..], Vec::as_slice)).collect();
        drawn_rows.push(draw_table_row(cont, &cells, &widths, &aligns));
    }
    // Sem régua, o olho se perde entre colunas afastadas ou células quebradas. Linha de dados
    // com a primeira célula vazia continua o grupo da anterior (convenção para agrupar), então
    // não ganha régua. Tabelas muito longas (dados) ficam compactas.
    let body = drawn_rows.len().saturating_sub(usize::from(t.has_header));
    let ruled = (2..=MAX_RULED_ROWS).contains(&body);
    let starts_group = |ri: usize| t.rows[ri].first().is_some_and(|c| c.iter().any(|p| p.width() > 0));
    let mut lines: Vec<Line<'static>> = vec![table_border(first, &widths, ["┌", "┬", "┐"])];
    let mut pinned = 1;
    let last = drawn_rows.len().saturating_sub(1);
    for (ri, drawn) in drawn_rows.into_iter().enumerate() {
        let is_head = ri == 0 && t.has_header;
        if is_head {
            pinned += drawn.len() + 1;
        }
        lines.extend(drawn);
        if is_head || (ruled && ri < last && starts_group(ri + 1)) {
            lines.push(table_border(cont, &widths, ["├", "┼", "┤"]));
        }
    }
    lines.push(table_border(cont, &widths, ["└", "┴", "┘"]));
    let hidden = total - shown;
    if hidden > 0 {
        let note = [Piece::Text(format!("… +{hidden} colunas"), Style::default().fg(DIM))];
        let mut v = cont.to_vec();
        v.extend(fit(&note, avail).0);
        lines.push(Line::from(v));
    }
    TableView { lines, pinned, first_col, shown_cols: shown, total_cols: total }
}

/// Maior largura de conteúdo de cada coluna de `idx` (mínimo 1).
pub(super) fn natural_widths(rows: &[Vec<Vec<Piece>>], idx: &[usize]) -> Vec<usize> {
    let mut natural = vec![1usize; idx.len()];
    for row in rows {
        for (n, &c) in natural.iter_mut().zip(idx) {
            if let Some(cell) = row.get(c) {
                *n = (*n).max(cell.iter().map(Piece::width).sum());
            }
        }
    }
    natural
}

/// Linha de borda da tabela; `corners` são o canto esquerdo, o cruzamento e o canto direito.
pub(super) fn table_border(prefix: &[Span<'static>], widths: &[usize], corners: [&str; 3]) -> Line<'static> {
    let [left, mid, right] = corners;
    let mut s = String::from(left);
    for (i, w) in widths.iter().enumerate() {
        s.push_str(&"─".repeat(w + 2));
        s.push_str(if i + 1 == widths.len() { right } else { mid });
    }
    let mut v = prefix.to_vec();
    v.push(Span::styled(s, Style::default().fg(DIM)));
    Line::from(v)
}

/// Linhas de dados da tabela: cada célula é quebrada na largura da sua coluna e alinhada;
/// a linha da tabela tem a altura da maior célula.
pub(super) fn draw_table_row(
    prefix: &[Span<'static>],
    row: &[&[Piece]],
    widths: &[usize],
    aligns: &[Alignment],
) -> Vec<Line<'static>> {
    let dim = Style::default().fg(DIM);
    let cells: Vec<Vec<(Vec<Span<'static>>, usize)>> = widths
        .iter()
        .enumerate()
                .map(|(c, &width)| wrap_cell(row.get(c).copied().unwrap_or(&[]), width))
        .collect();
    let height = cells.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|li| {
            let mut v = prefix.to_vec();
            v.push(Span::styled("│", dim));
            for (c, &width) in widths.iter().enumerate() {
                let (spans, w) = cells[c].get(li).cloned().unwrap_or_default();
                let pad = width.saturating_sub(w);
                let (lead, trail) = match aligns.get(c).copied().unwrap_or(Alignment::None) {
                    Alignment::Right => (pad, 0),
                    Alignment::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                v.push(Span::raw(format!(" {}", " ".repeat(lead))));
                v.extend(spans);
                v.push(Span::raw(format!("{} ", " ".repeat(trail))));
                v.push(Span::styled("│", dim));
            }
            Line::from(v)
        })
        .collect()
}

/// Reduz as colunas mais largas até a tabela caber em `avail`, sem passar de
/// `MIN_COL_WIDTH` por coluna. Equivale a tirar 1 coluna da mais larga por vez
/// (empates saem da última), mas em O(n log largura) em vez de O(n * largura).
pub(super) fn compute_column_widths(natural: &[usize], avail: usize) -> Vec<usize> {
    let budget = avail.saturating_sub(CELL_OVERHEAD * natural.len() + 1);
    if natural.iter().sum::<usize>() <= budget {
        return natural.to_vec();
    }
    let capped_sum = |cap: usize| -> usize { natural.iter().map(|&w| w.min(cap)).sum() };
    let widest = natural.iter().copied().max().unwrap_or(0);
    let cap = largest_cap_within(MIN_COL_WIDTH, widest, budget, capped_sum);
    let mut leftover = budget.saturating_sub(capped_sum(cap));
    natural
        .iter()
        .map(|&w| {
            if w > cap && leftover > 0 {
                leftover -= 1;
                cap + 1
            } else {
                w.min(cap)
            }
        })
        .collect()
}

/// Maior teto em `lo..=hi` cuja soma ainda cabe no orçamento (`lo` se nenhum couber).
pub(super) fn largest_cap_within(mut lo: usize, mut hi: usize, budget: usize, sum: impl Fn(usize) -> usize) -> usize {
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if sum(mid) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// Mantém só os primeiros `budget` colunas da célula; o `bool` diz se algo foi cortado.
/// Evita quebrar em linhas um texto de milhões de caracteres que só mostraria `MAX_CELL_LINES`.
pub(super) fn clip_pieces(cell: &[Piece], budget: usize) -> (Vec<Piece>, bool) {
    let mut used = 0;
    let mut out = Vec::new();
    for piece in cell {
        let text = piece.as_text().0;
        let room = budget - used;
        let w = text.width();
        if w <= room {
            out.push(with_text(piece, text.to_string()));
            used += w;
            continue;
        }
        let mut buf = String::new();
        let mut taken = 0;
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if taken + cw > room {
                break;
            }
            buf.push(ch);
            taken += cw;
        }
        if !buf.is_empty() {
            out.push(with_text(piece, buf));
        }
        return (out, true);
    }
    (out, false)
}

/// Quebra a célula em linhas de até `w` colunas (no máximo `MAX_CELL_LINES`), cada uma com a
/// sua largura. Se sobrar texto, a última linha termina em "…".
pub(super) fn wrap_cell(cell: &[Piece], w: usize) -> Vec<(Vec<Span<'static>>, usize)> {
    let w = w.max(1);
    let (pieces, clipped) = clip_pieces(cell, w.saturating_mul(MAX_CELL_LINES));
    let mut lines = wrap(&pieces, w);
    let overflow = lines.len() > MAX_CELL_LINES;
    lines.truncate(MAX_CELL_LINES);
    if overflow || clipped {
        let last = lines.pop().unwrap_or_default();
        let style = last.last().map_or_else(Style::default, |s| s.style);
        let mut ps: Vec<Piece> =
            last.iter().map(|s| Piece::Text(s.content.to_string(), s.style)).collect();
        ps.push(Piece::Text("…".to_string(), style));
        lines.push(fit(&ps, w).0);
    }
    lines
        .into_iter()
        .map(|spans| {
            let n = spans_width(&spans);
            (spans, n)
        })
        .collect()
}

/// Corta a célula para caber em `w` colunas, usando "…" quando precisa truncar.
pub(super) fn fit(cell: &[Piece], w: usize) -> (Vec<Span<'static>>, usize) {
    let total: usize = cell.iter().map(Piece::width).sum();
    if total <= w {
        let spans = cell
            .iter()
            .map(|p| {
                let (t, st) = p.as_text();
                Span::styled(t.to_string(), st)
            })
            .collect();
        return (spans, total);
    }
    let limit = w.saturating_sub(1);
    let mut used = 0;
    let mut out: Vec<Span<'static>> = Vec::new();
    'outer: for piece in cell {
        let (text, st) = piece.as_text();
        let mut buf = String::new();
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > limit {
                if !buf.is_empty() {
                    out.push(Span::styled(buf, st));
                }
                out.push(Span::styled("…", st));
                used += 1;
                break 'outer;
            }
            buf.push(ch);
            used += cw;
        }
        out.push(Span::styled(buf, st));
    }
    (out, used)
}
