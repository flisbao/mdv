//! Desenho do quadro: documento, barra de status e textos auxiliares.

use super::*;

/// Colunas deixadas livres nas laterais da TUI.
pub(crate) const TUI_MARGIN: usize = 4;

/// Menor largura de texto na TUI, mesmo em terminais minúsculos.
pub(crate) const MIN_TUI_WIDTH: usize = 10;

impl App {
    /// As linhas da janela como serão desenhadas: com o destaque da busca.
    pub(crate) fn screen_lines(&self) -> Vec<Line<'static>> {
        let q = self.search.query.as_str();
        self.doc.rendered.lines[self.window()]
            .iter()
            .map(|l| {
                let mut l = if q.is_empty() {
                    l.clone()
                } else {
                    highlight_line(l, q)
                };
                self.decorate_links(&mut l);
                l
            })
            .collect()
    }

    /// Largura do texto para uma tela de `screen` colunas.
    pub(crate) fn text_width(&self, screen: u16) -> usize {
        usize::from(screen)
            .saturating_sub(TUI_MARGIN)
            .min(self.max_width)
            .max(MIN_TUI_WIDTH)
    }

    pub(crate) fn draw(&mut self, f: &mut Frame) {
        if self.picker.is_some() {
            self.draw_picker(f);
            return;
        }
        let area = f.area();
        let [content, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let w = self.text_width(content.width);
        if self.doc.needs_render(w) {
            self.rerender(w);
        }
        self.open_pending_table();
        self.apply_pending_jump();
        if self.img_zoom.is_some() {
            self.draw_img_zoom(f, content, status);
            return;
        }
        if self.zoom.is_some() {
            self.draw_zoom(f, content, status);
            return;
        }
        self.view.height = usize::from(content.height);
        self.view.scroll = self
            .view
            .scroll
            .min(self.view.max_scroll(self.total_lines()));
        self.apply_stick_bottom();

        let pad = usize::from(content.width).saturating_sub(w) / 2;
        let inner = Rect {
            x: content
                .x
                .saturating_add(u16::try_from(pad).unwrap_or(u16::MAX)),
            y: content.y,
            width: u16::try_from(w).unwrap_or(u16::MAX).min(content.width),
            height: content.height,
        };
        let visible = self.screen_lines();
        f.render_widget(Paragraph::new(visible), inner);

        let bar = Style::default()
            .bg(Color::Rgb(59, 66, 82))
            .fg(Color::Rgb(229, 233, 240));
        f.render_widget(
            Paragraph::new(self.status_line(usize::from(status.width))).style(bar),
            status,
        );
    }

    pub(crate) fn status_line(&self, width: usize) -> Line<'static> {
        let left = match &self.search.input {
            Some(buf) => format!(" /{buf}█"),
            None => self.status_left(),
        };
        let right = "j/k rolar  [ ] título  / buscar  Tab link  ⌫ voltar  t tabela  y/Y copiar  i imagem  q sair ";
        let (lw, rw) = (left.width(), right.width());
        let text = if lw + rw < width {
            format!("{left}{}{right}", " ".repeat(width - lw - rw))
        } else {
            left
        };
        Line::from(text)
    }

    /// `☑ feitas/total` do documento e, se a seção atual tem tarefas e difere do todo, `(seção f/t)`.
    pub(crate) fn task_summary(&self) -> Option<String> {
        let all = render::count_tasks(&self.doc.rendered.tasks, 0, usize::MAX);
        if all.total == 0 {
            return None;
        }
        let mut s = format!("☑ {}/{}", all.done, all.total);
        if let Some(idx) = render::section_at(&self.doc.rendered.outline, self.view.scroll) {
            let c = render::section_tasks(&self.doc.rendered, idx);
            if c.total > 0 && c != all {
                let _ = write!(s, " (seção {}/{})", c.done, c.total);
            }
        }
        Some(s)
    }

    pub(crate) fn status_left(&self) -> String {
        let total = self.total_lines();
        let pct = self
            .view
            .scroll
            .saturating_mul(100)
            .checked_div(self.view.max_scroll(total))
            .unwrap_or(100);
        let view = match (self.doc.opts.mode, self.doc.opts.json_view) {
            (cli::Mode::Json, cli::JsonView::Text) => "  [texto]",
            (cli::Mode::Json, cli::JsonView::Table) => "  [tabela]",
            (cli::Mode::Json, cli::JsonView::Schema) => "  [esquema]",
            _ => "",
        };
        let mut s = format!(
            " {}  {pct}%{}{view}",
            self.name,
            if self.raw { "  [fonte]" } else { "" }
        );
        if let Some(c) = self.doc.opts.code_scroll {
            let _ = write!(s, "  [código sem quebra, col {c}]");
        }
        let search = &self.search;
        if !search.query.is_empty() {
            if search.matches.is_empty() {
                let _ = write!(s, "  /{}: nada encontrado", search.query);
            } else {
                let _ = write!(
                    s,
                    "  /{} [{}/{}]",
                    search.query,
                    search.current + 1,
                    search.matches.len()
                );
            }
        }
        if let Some(t) = self.task_summary() {
            let _ = write!(s, "  {t}");
        }
        if let Some(n) = &self.notice {
            let _ = write!(s, "  {n}");
        }
        s
    }
}
