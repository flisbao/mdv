//! Zoom de tabela (`t`): uma tabela desenhada na largura inteira do terminal.

use super::*;

/// Tela de zoom: uma tabela desenhada na largura inteira do terminal.
pub(crate) struct Zoom {
    /// Índice em `Rendered.tables`.
    pub(crate) table: usize,
    /// Primeira coluna rolável pedida (a coluna 0 fica sempre; `draw` normaliza).
    pub(crate) first_col: usize,
    /// Rolagem do corpo da tabela, sem as linhas fixas do topo.
    pub(crate) view: Viewport,
    /// Último desenho: (largura, `first_col` usado, resultado).
    pub(crate) drawn: Option<(usize, usize, render::TableView)>,
}

impl Zoom {
    pub(crate) fn new(table: usize) -> Self {
        Zoom {
            table,
            first_col: 0,
            view: Viewport::default(),
            drawn: None,
        }
    }

    /// Linhas do corpo (tudo que não é fixo no topo).
    pub(crate) fn body_total(&self) -> usize {
        self.drawn
            .as_ref()
            .map_or(0, |(_, _, v)| v.lines.len().saturating_sub(v.pinned))
    }
}

/// Índice do intervalo `[start, end)` com mais linhas dentro da janela (empate: o primeiro).
/// Escolher o primeiro que toca a janela esconde os blocos seguintes quando um é mais alto que a tela.
pub(crate) fn most_visible(
    spans: impl Iterator<Item = (usize, usize)>,
    window: std::ops::Range<usize>,
) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for (i, (start, end)) in spans.enumerate() {
        let seen = end.min(window.end).saturating_sub(start.max(window.start));
        if seen > 0 && best.is_none_or(|(_, n)| seen > n) {
            best = Some((i, seen));
        }
    }
    best.map(|(i, _)| i)
}

/// Texto do título da seção que contém a linha renderizada `line`.
pub(crate) fn section_title(r: &render::Rendered, line: usize) -> Option<String> {
    render::section_at(&r.outline, line).map(|i| render::sanitize(&r.outline[i].title).into_owned())
}

/// Próximo/anterior índice entre `n` itens, dando a volta nos extremos; `None` com menos de 2 itens.
pub(crate) fn step_index(cur: usize, n: usize, forward: bool) -> Option<usize> {
    if n < 2 {
        return None;
    }
    Some(if forward {
        (cur + 1) % n
    } else {
        (cur + n - 1) % n
    })
}

/// "todas as N colunas" ou "colunas 1 e A–B de N" (1 é a coluna fixa).
pub(crate) fn columns_label(v: &render::TableView) -> String {
    if v.total_cols == 1 {
        return "1 coluna".to_string();
    }
    if v.shown_cols >= v.total_cols {
        return format!("todas as {} colunas", v.total_cols);
    }
    if v.shown_cols == 1 {
        return format!("coluna 1 de {}", v.total_cols);
    }
    let (from, to) = (v.first_col + 1, v.first_col + v.shown_cols - 1);
    format!("colunas 1 e {from}–{to} de {}", v.total_cols)
}

impl App {
    /// Abre o zoom pedido por `--table`, assim que há um documento renderizado.
    pub(crate) fn open_pending_table(&mut self) {
        if let Some(i) = self.open_table.take() {
            self.open_zoom(i);
        }
    }

    pub(crate) fn open_zoom(&mut self, table: usize) {
        if table >= self.doc.rendered.tables.len() {
            self.notice = Some(format!(
                "tabela {} não existe (há {})",
                table + 1,
                self.doc.rendered.tables.len()
            ));
            return;
        }
        self.notice = None;
        self.modal = Modal::TableZoom(Zoom::new(table));
    }

    /// Abre o zoom da primeira tabela que aparece na tela.
    pub(crate) fn open_visible_table(&mut self) {
        let achada = most_visible(
            self.doc.rendered.tables.iter().map(|t| (t.start, t.end)),
            self.window(),
        );
        match achada {
            Some(i) => self.open_zoom(i),
            None => self.notice = Some("nenhuma tabela na tela".to_string()),
        }
    }

    /// Redesenha a tabela do zoom se a largura ou a coluna mudaram.
    pub(crate) fn refresh_zoom(&mut self, width: usize) {
        let Modal::TableZoom(z) = &mut self.modal else {
            return;
        };
        let Some(span) = self.doc.rendered.tables.get(z.table) else {
            return;
        };
        let fresh = matches!(&z.drawn, Some((w, c, _)) if *w == width && *c == z.first_col);
        if !fresh {
            let v = span.data.draw(width, z.first_col);
            z.first_col = v.first_col;
            z.drawn = Some((width, v.first_col, v));
        }
    }

    pub(crate) fn on_zoom_key(&mut self, code: KeyCode) {
        let Modal::TableZoom(z) = &mut self.modal else {
            return;
        };
        let body = z.body_total();
        let h = z.view.height;
        let page = isize::try_from(h.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
        let half = isize::try_from((h / 2).max(1)).unwrap_or(isize::MAX);
        match code {
            KeyCode::Esc | KeyCode::Char('q' | 't') => self.modal = Modal::None,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => z.view.scroll_by(1, body),
            KeyCode::Char('k') | KeyCode::Up => z.view.scroll_by(-1, body),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => z.view.scroll_by(page, body),
            KeyCode::Char('b') | KeyCode::PageUp => z.view.scroll_by(-page, body),
            KeyCode::Char('d') => z.view.scroll_by(half, body),
            KeyCode::Char('u') => z.view.scroll_by(-half, body),
            KeyCode::Char('g') | KeyCode::Home => z.view.scroll_to(0, body),
            KeyCode::Char('G') | KeyCode::End => z.view.scroll_to(usize::MAX, body),
            KeyCode::Char('l') | KeyCode::Right => z.first_col = z.first_col.saturating_add(1),
            KeyCode::Char('h') | KeyCode::Left => z.first_col = z.first_col.saturating_sub(1),
            KeyCode::Char('0') => z.first_col = 0,
            KeyCode::Char('$') => z.first_col = usize::MAX,
            KeyCode::Char(c @ ('n' | 'p')) => {
                if let Some(next) = step_index(z.table, self.doc.rendered.tables.len(), c == 'n') {
                    *z = Zoom::new(next);
                }
            }
            _ => {}
        }
    }

    /// Nome do arquivo, posição no documento e estado da busca.
    pub(crate) fn draw_zoom(&mut self, f: &mut Frame, content: Rect, status: Rect) {
        self.refresh_zoom(render::clamp_width(usize::from(content.width)));
        let Modal::TableZoom(z) = &mut self.modal else {
            return;
        };
        let Some((_, _, tv)) = &z.drawn else { return };
        let pinned = tv.pinned.min(tv.lines.len());
        z.view.height = usize::from(content.height).saturating_sub(pinned).max(1);
        let body = tv.lines.len() - pinned;
        z.view.scroll = z.view.scroll.min(z.view.max_scroll(body));
        let start = pinned + z.view.scroll;
        let end = (start + z.view.height).min(tv.lines.len());
        let mut visible: Vec<Line<'static>> = tv.lines[..pinned].to_vec();
        visible.extend_from_slice(&tv.lines[start..end]);
        visible.iter_mut().for_each(render::strip_link_marks);
        f.render_widget(Paragraph::new(visible), content);

        let bar = Style::default()
            .bg(Color::Rgb(59, 66, 82))
            .fg(Color::Rgb(229, 233, 240));
        let titulo = self
            .doc
            .rendered
            .tables
            .get(z.table)
            .and_then(|t| section_title(&self.doc.rendered, t.start))
            .map_or(String::new(), |t| format!("  «{t}»"));
        let left = format!(
            " {}  tabela {}/{}{titulo}  {}",
            self.name,
            z.table + 1,
            self.doc.rendered.tables.len(),
            columns_label(tv)
        );
        let right = "j/k rolar  h/l colunas  n/p outra tabela  Esc voltar ";
        let width = usize::from(status.width);
        let (lw, rw) = (left.width(), right.width());
        let text = if lw + rw < width {
            format!("{left}{}{right}", " ".repeat(width - lw - rw))
        } else {
            left
        };
        f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
    }
}
