//! O `Renderer`: máquina de estados sobre os eventos do `pulldown-cmark`.

use super::*;

/// Imagem Markdown em andamento: destino e posição em `inline` do "[imagem: " de fallback.
pub(super) struct Renderer<'a> {
    pub(super) width: usize,
    pub(super) out: Rendered,
    pub(super) inline: InlineBuffer,
    pub(super) lists: ListStack,
    pub(super) quote_depth: usize,
    pub(super) code: Option<CodeBlock>,
    pub(super) table: Option<TableBuilder>,
    pub(super) highlight_left: usize,
    pub(super) images: Option<&'a dyn ImageProvider>,
    pub(super) image: Option<ImageCtx>,
    pub(super) src_line: usize,
    pub(super) code_scroll: Option<usize>,
    /// Imagens vistas dentro de células (alt, imagem): desenhadas depois da tabela.
    pub(super) table_figures: Vec<(String, Arc<image::DynamicImage>)>,
}

impl<'a> Renderer<'a> {
    pub(super) fn new(width: usize, images: Option<&'a dyn ImageProvider>) -> Self {
        Renderer {
            images,
            image: None,
            src_line: 0,
            table_figures: Vec::new(),
            width: clamp_width(width),
            out: Rendered::default(),
            inline: InlineBuffer::default(),
            lists: ListStack::default(),
            quote_depth: 0,
            code: None,
            table: None,
            highlight_left: MAX_HIGHLIGHT_BYTES,
            code_scroll: None,
        }
    }

    pub(super) fn finish(mut self) -> Rendered {
        self.flush_inline();
        while self.out.lines.last().is_some_and(is_blank) {
            self.out.lines.pop();
        }
        let n = self.out.lines.len();
        self.out.line_src.resize(n, self.src_line);
        self.out
    }

    pub(super) fn handle(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match &mut self.code {
                Some(code) => code.buf.push_str(&t),
                None => self.inline.push_text(&t),
            },
            Event::Code(c) => {
                let style = self
                    .inline
                    .style()
                    .patch(Style::default().fg(INLINE_CODE_FG).bg(INLINE_CODE_BG));
                self.inline.push(Piece::Code(c.to_string(), style));
            }
            Event::SoftBreak => self.inline.push_text(" "),
            Event::HardBreak => self.inline.push(Piece::Break),
            Event::Rule => self.rule(),
            Event::TaskListMarker(done) => {
                self.out.tasks.push(TaskItem { line: self.out.lines.len(), done });
                self.lists.set_task_marker(done);
            }
            Event::Html(h) => self.html_block(&h),
            Event::InlineHtml(h) => self.inline.push_dim(h.to_string()),
            Event::FootnoteReference(label) => self.inline.push_dim(format!("[^{label}]")),
            _ => {}
        }
    }

    pub(super) fn rule(&mut self) {
        self.flush_inline();
        self.push_blank();
        let rule = "─".repeat(self.content_width());
        self.push_prefixed(vec![vec![Span::styled(rule, Style::default().fg(DIM))]]);
        self.push_blank();
    }

    pub(super) fn html_block(&mut self, html: &str) {
        self.flush_inline();
        let dim = Style::default().fg(DIM);
        let avail = self.content_width();
        let mut bodies: Vec<Vec<Span<'static>>> = Vec::new();
        let text = |s: String, st: Style, bodies: &mut Vec<Vec<Span<'static>>>| {
            for chunk in split_by_width(&[(s, st)], avail) {
                bodies.push(chunk.into_iter().map(|(t, st)| Span::styled(t, st)).collect());
            }
        };
        for l in html.lines() {
            let t = l.trim();
            if t.starts_with("<details") || t == "</details>" {
                continue;
            }
            if let Some(s) = summary_text(t) {
                text(format!("▸ {s}"), Style::default().fg(H2).add_modifier(Modifier::BOLD), &mut bodies);
            } else {
                text(l.to_string(), dim, &mut bodies);
            }
        }
        self.push_prefixed(bodies);
    }

    pub(super) fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote(kind) => {
                self.flush_inline();
                self.quote_depth += 1;
                self.inline
                    .push_style(Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC));
                if let Some(kind) = kind {
                    let (label, color) = match kind {
                        BlockQuoteKind::Note => ("ℹ Nota", H2),
                        BlockQuoteKind::Tip => ("💡 Dica", H3),
                        BlockQuoteKind::Important => ("❗ Importante", H1),
                        BlockQuoteKind::Warning => ("⚠ Aviso", Color::Rgb(240, 192, 64)),
                        BlockQuoteKind::Caution => ("⛔ Cuidado", Color::Rgb(248, 81, 73)),
                    };
                    let mut line = self.quote_prefix();
                    line.push(Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)));
                    self.out.lines.push(Line::from(line));
                }
            }
            Tag::MetadataBlock(_) => {
                self.flush_inline();
                self.code = Some(CodeBlock::new(CodeBlockKind::Fenced("yaml".into())));
            }
            Tag::FootnoteDefinition(label) => {
                self.flush_inline();
                self.inline.push_dim(format!("[^{label}]: "));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                self.code = Some(CodeBlock::new(kind));
            }
            Tag::List(start) => {
                self.flush_inline();
                self.lists.open(start);
            }
            Tag::Item => {
                self.flush_inline();
                self.lists.start_item();
            }
            Tag::Emphasis => self.inline.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.inline.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .inline
                .push_style(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.inline.links.push(dest_url.to_string());
                let mut style = Style::default().fg(LINK).add_modifier(Modifier::UNDERLINED);
                if !dest_url.is_empty() && self.out.links.len() < MAX_LINKS {
                    style = style.underline_color(link_mark(self.out.links.len()));
                    self.out.links.push(dest_url.to_string());
                }
                self.inline.push_style(style);
            }
            Tag::Image { dest_url, .. } => {
                self.inline.push_style(Style::default().add_modifier(Modifier::ITALIC));
                self.image = Some(ImageCtx { url: dest_url.to_string(), mark: self.inline.pieces.len() });
                self.inline.push_dim("[imagem: ");
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.table = Some(TableBuilder::new(aligns));
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.begin_head();
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.begin_row();
                }
            }
            Tag::TableCell if self.table.as_ref().is_some_and(|t| t.in_head) => {
                self.inline
                    .push_style(Style::default().fg(H2).add_modifier(Modifier::BOLD));
            }
            _ => {}
        }
    }

    pub(super) fn start_heading(&mut self, level: HeadingLevel) {
        self.flush_inline();
        self.push_blank();
        let color = match level {
            HeadingLevel::H1 => H1,
            HeadingLevel::H2 => H2,
            HeadingLevel::H3 => H3,
            _ => H4,
        };
        self.inline
            .push_style(Style::default().fg(color).add_modifier(Modifier::BOLD));
    }

    pub(super) fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(level) => self.finish_heading(level),
            TagEnd::BlockQuote(_) => self.end_blockquote(),
            TagEnd::CodeBlock | TagEnd::MetadataBlock(_) => {
                if let Some(code) = self.code.take() {
                    self.render_code(&code);
                }
                if self.lists.is_empty() {
                    self.push_blank();
                }
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.close();
                if self.lists.is_empty() {
                    self.push_blank();
                }
            }
            TagEnd::Item | TagEnd::FootnoteDefinition => self.flush_inline(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.inline.pop_style(),
            TagEnd::Link => self.end_link(),
            TagEnd::Image => self.end_image(),
            TagEnd::TableCell => self.end_table_cell(),
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.end_head();
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    t.end_row();
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
                self.flush_table_figures();
                self.push_blank();
            }
            _ => {}
        }
    }

    pub(super) fn end_paragraph(&mut self) {
        self.flush_inline();
        if !self.lists.is_empty() {
            return;
        }
        if self.quote_depth > 0 {
            let bar = self.quote_prefix();
            self.out.lines.push(Line::from(bar));
        } else {
            self.push_blank();
        }
    }

    pub(super) fn end_blockquote(&mut self) {
        self.flush_inline();
        self.quote_depth = self.quote_depth.saturating_sub(1);
        self.inline.pop_style();
        if self.quote_depth > 0 {
            return;
        }
        let only_bar = self.out.lines.last().is_some_and(|l| {
            !l.spans.is_empty()
                && l.spans
                    .iter()
                    .all(|s| s.content == QUOTE_BAR && s.style.fg == Some(QUOTE_BAR_COLOR))
        });
        if only_bar {
            self.out.lines.pop();
        }
        self.push_blank();
    }

    pub(super) fn end_link(&mut self) {
        self.inline.pop_style();
        if let Some(url) = self.inline.links.pop() {
            if !url.is_empty() && !url.starts_with('#') {
                self.inline.push_dim(format!(" ({url})"));
            }
        }
    }

    pub(super) fn end_table_cell(&mut self) {
        let cell = self.inline.take();
        let Some(t) = &mut self.table else { return };
        t.push_cell(cell);
        if t.in_head {
            self.inline.pop_style();
        }
    }

    pub(super) fn push_blank(&mut self) {
        match self.out.lines.last() {
            None => {}
            Some(l) if is_blank(l) => {}
            Some(_) => self.out.lines.push(Line::default()),
        }
    }

    pub(super) fn quote_prefix(&self) -> Vec<Span<'static>> {
        (0..self.quote_depth)
            .map(|_| Span::styled(QUOTE_BAR, Style::default().fg(QUOTE_BAR_COLOR)))
            .collect()
    }

    /// Prefixos (primeira linha, linhas seguintes) com citação e indentação de listas.
    /// Não altera o estado: quem desenha a primeira linha chama `take_marker`.
    pub(super) fn prefixes(&self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = self.quote_prefix();
        let mut cont = self.quote_prefix();
        self.lists.extend_prefixes(&mut first, &mut cont);
        (first, cont)
    }

    pub(super) fn take_marker(&mut self) {
        self.lists.take_markers();
    }

    /// Colunas disponíveis para o conteúdo depois dos prefixos de citação e lista.
    pub(super) fn content_width(&self) -> usize {
        let (first, _) = self.prefixes();
        self.width.saturating_sub(spans_width(&first)).max(1)
    }

    /// Acrescenta linhas já quebradas à saída, com os prefixos de citação e lista
    /// (o marcador do item só na primeira).
    pub(super) fn push_prefixed(&mut self, bodies: Vec<Vec<Span<'static>>>) {
        let (first, cont) = self.prefixes();
        self.take_marker();
        for (i, body) in bodies.into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(body);
            self.out.lines.push(Line::from(v));
        }
    }

    pub(super) fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let toks = self.inline.take();
        let (first, cont) = self.prefixes();
        self.take_marker();
        let avail = self.width.saturating_sub(spans_width(&first)).max(MIN_BODY_WIDTH);
        for (i, spans) in wrap(&toks, avail).into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(spans);
            self.out.lines.push(Line::from(v));
        }
    }

    pub(super) fn finish_heading(&mut self, level: HeadingLevel) {
        let toks = self.inline.take();
        self.inline.pop_style();
        self.out.headings.push(self.out.lines.len());
        self.out.outline.push(OutlineEntry {
            line: self.out.lines.len(),
            level: level as u8,
            title: toks.iter().map(|p| p.as_text().0).collect(),
        });
        let (marker, color) = match level {
            HeadingLevel::H2 => ("▌ ", H2),
            HeadingLevel::H3 => (QUOTE_BAR, H3),
            _ => ("", H1),
        };
        let mw = marker.width();
        let avail = self.content_width();
        let mut bodies: Vec<Vec<Span<'static>>> = Vec::new();
        for (i, spans) in wrap(&toks, avail.saturating_sub(mw).max(1)).into_iter().enumerate() {
            let mut v: Vec<Span<'static>> = Vec::new();
            if mw > 0 {
                v.push(if i == 0 {
                    Span::styled(marker, Style::default().fg(color))
                } else {
                    Span::raw(" ".repeat(mw))
                });
            }
            v.extend(spans);
            bodies.push(v);
        }
        if matches!(level, HeadingLevel::H1) {
            let rule = "═".repeat(avail);
            bodies.push(vec![Span::styled(rule, Style::default().fg(DIM))]);
        }
        self.push_prefixed(bodies);
        self.push_blank();
    }

    pub(super) fn render_table(&mut self, t: TableBuilder) {
        let (first, cont) = self.prefixes();
        let avail = self.width.saturating_sub(spans_width(&cont));
        let data = TableData { rows: t.rows, aligns: t.aligns, has_header: t.has_header };
        if data.total_cols() == 0 {
            return;
        }
        self.take_marker();
        let view = layout_table(&data, &first, &cont, avail, 0);
        let start = self.out.lines.len();
        let end = start + view.lines.len();
        self.out.lines.extend(view.lines);
        self.out.tables.push(TableSpan { start, end, data });
    }
}
