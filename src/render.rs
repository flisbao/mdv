//! Converte Markdown em linhas estilizadas (ratatui) já quebradas na largura pedida.
//!
//! O mesmo resultado serve para os dois modos do `mdv`: a TUI desenha as linhas
//! direto no `ratatui` e o modo `--plain` converte para ANSI e imprime.

use std::borrow::Cow;
use std::sync::OnceLock;

use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const H1: Color = Color::Rgb(210, 168, 255);
const H2: Color = Color::Rgb(121, 192, 255);
const H3: Color = Color::Rgb(126, 231, 135);
const H4: Color = Color::Rgb(230, 237, 243);
const DIM: Color = Color::Rgb(110, 118, 129);
const LINK: Color = Color::Rgb(88, 166, 255);
const INLINE_CODE_FG: Color = Color::Rgb(255, 166, 87);
const INLINE_CODE_BG: Color = Color::Rgb(47, 52, 60);
const CODE_BG: Color = Color::Rgb(40, 44, 52);
const QUOTE: Color = Color::Rgb(139, 148, 158);
const BULLET: Color = Color::Rgb(126, 231, 135);

/// Limites de largura de renderização (evitam alocações gigantes em `repeat`).
pub const MIN_WIDTH: usize = 20;
pub const MAX_WIDTH: usize = 1000;

const REPLACEMENT: char = '\u{FFFD}';

/// Trecho de texto com estilo.
type Styled = (String, Style);

/// Item do texto em linha: trecho estilizado ou quebra de linha forçada.
#[derive(Clone)]
enum Piece {
    Text(String, Style),
    /// Código em linha: os espaços são literais (não colapsam nem quebram a linha).
    Code(String, Style),
    Break,
}

impl Piece {
    /// Texto e estilo para contextos sem quebra de linha (células de tabela): `Break` vira espaço.
    fn as_text(&self) -> (&str, Style) {
        match self {
            Piece::Text(t, st) | Piece::Code(t, st) => (t, *st),
            Piece::Break => (" ", Style::default()),
        }
    }

    fn width(&self) -> usize {
        self.as_text().0.width()
    }
}

/// Resultado da renderização.
#[derive(Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    /// Índices (em `lines`) onde cada título começa.
    pub headings: Vec<usize>,
}

pub fn clamp_width(width: usize) -> usize {
    width.clamp(MIN_WIDTH, MAX_WIDTH)
}

pub fn render(md: &str, width: usize) -> Rendered {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer {
        width: clamp_width(width),
        lines: Vec::new(),
        headings: Vec::new(),
        inline: Vec::new(),
        styles: Vec::new(),
        lists: Vec::new(),
        quote_depth: 0,
        heading: None,
        code: None,
        code_buf: String::new(),
        links: Vec::new(),
        table: None,
    };
    for ev in Parser::new_ext(md, opts) {
        r.handle(sanitize_event(ev));
    }
    r.flush_inline();
    while r.lines.last().is_some_and(is_blank) {
        r.lines.pop();
    }
    Rendered { lines: r.lines, headings: r.headings }
}

/// Ponto único de entrada do texto do documento: troca caracteres de controle
/// (ESC, BEL, C1, `\r` solto...) por U+FFFD, para que nada do Markdown vire
/// comando de terminal. Preserva `\n` e `\t`; `\r\n` vira `\n`.
fn sanitize(s: &str) -> Cow<'_, str> {
    let is_unsafe = |c: char| c.is_control() && c != '\n' && c != '\t';
    if !s.contains(is_unsafe) {
        return Cow::Borrowed(s);
    }
    let unified = s.replace("\r\n", "\n");
    Cow::Owned(
        unified
            .chars()
            .map(|c| if is_unsafe(c) { REPLACEMENT } else { c })
            .collect(),
    )
}

fn sanitize_cow(s: CowStr<'_>) -> CowStr<'_> {
    match sanitize(&s) {
        Cow::Borrowed(_) => s,
        Cow::Owned(clean) => CowStr::from(clean),
    }
}

/// Sanitiza todo texto vindo do documento que chega a `Renderer::handle`.
fn sanitize_event(ev: Event<'_>) -> Event<'_> {
    match ev {
        Event::Text(t) => Event::Text(sanitize_cow(t)),
        Event::Code(t) => Event::Code(sanitize_cow(t)),
        Event::Html(t) => Event::Html(sanitize_cow(t)),
        Event::InlineHtml(t) => Event::InlineHtml(sanitize_cow(t)),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link {
            link_type,
            dest_url: sanitize_cow(dest_url),
            title,
            id,
        }),
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(sanitize_cow(lang))))
        }
        other => other,
    }
}

/// Converte linhas estilizadas em texto com escapes ANSI (ou texto puro se `color` for falso).
pub fn to_ansi(lines: &[Line<'_>], color: bool) -> String {
    let mut out = String::new();
    for line in lines {
        for span in &line.spans {
            let codes = if color { sgr(&span.style) } else { String::new() };
            if codes.is_empty() {
                out.push_str(&span.content);
            } else {
                out.push_str(&format!("\x1b[{codes}m{}\x1b[0m", span.content));
            }
        }
        out.push('\n');
    }
    out
}

fn sgr(st: &Style) -> String {
    let mut v: Vec<String> = Vec::new();
    let m = st.add_modifier;
    if m.contains(Modifier::BOLD) {
        v.push("1".into());
    }
    if m.contains(Modifier::DIM) {
        v.push("2".into());
    }
    if m.contains(Modifier::ITALIC) {
        v.push("3".into());
    }
    if m.contains(Modifier::UNDERLINED) {
        v.push("4".into());
    }
    if m.contains(Modifier::CROSSED_OUT) {
        v.push("9".into());
    }
    if let Some(c) = st.fg.and_then(|c| color_code(c, false)) {
        v.push(c);
    }
    if let Some(c) = st.bg.and_then(|c| color_code(c, true)) {
        v.push(c);
    }
    v.join(";")
}

fn color_code(c: Color, bg: bool) -> Option<String> {
    let base = if bg { 48 } else { 38 };
    match c {
        Color::Rgb(r, g, b) => Some(format!("{base};2;{r};{g};{b}")),
        Color::Indexed(i) => Some(format!("{base};5;{i}")),
        _ => None,
    }
}

fn hl() -> &'static (SyntaxSet, Theme) {
    static HL: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    HL.get_or_init(|| {
        let ss = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        let theme = ts.themes["base16-ocean.dark"].clone();
        (ss, theme)
    })
}

fn is_blank(l: &Line<'_>) -> bool {
    l.spans.iter().all(|s| s.content.trim().is_empty())
}

struct ListLevel {
    ordered: Option<u64>,
    /// Largura do marcador do item atual (usada para indentar o conteúdo).
    width: usize,
    /// Marcador ainda não impresso (sai junto com a primeira linha do item).
    pending: Option<(String, Style)>,
}

struct TableState {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Piece>>>,
    cur: Vec<Vec<Piece>>,
    header: bool,
    in_head: bool,
}

struct Renderer {
    width: usize,
    lines: Vec<Line<'static>>,
    headings: Vec<usize>,
    inline: Vec<Piece>,
    styles: Vec<Style>,
    lists: Vec<ListLevel>,
    quote_depth: usize,
    heading: Option<HeadingLevel>,
    code: Option<String>,
    code_buf: String,
    links: Vec<String>,
    table: Option<TableState>,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.iter().fold(Style::default(), |a, s| a.patch(*s))
    }

    fn handle(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if self.code.is_some() {
                    self.code_buf.push_str(&t);
                } else {
                    let st = self.style();
                    self.inline.push(Piece::Text(t.to_string(), st));
                }
            }
            Event::Code(c) => {
                let st = self
                    .style()
                    .patch(Style::default().fg(INLINE_CODE_FG).bg(INLINE_CODE_BG));
                self.inline.push(Piece::Code(c.to_string(), st));
            }
            Event::SoftBreak => {
                let st = self.style();
                self.inline.push(Piece::Text(" ".into(), st));
            }
            Event::HardBreak => {
                self.inline.push(Piece::Break);
            }
            Event::Rule => {
                self.flush_inline();
                self.push_blank();
                let rule = "─".repeat(self.content_width());
                self.push_prefixed(vec![vec![Span::styled(rule, Style::default().fg(DIM))]]);
                self.push_blank();
            }
            Event::TaskListMarker(done) => {
                if let Some(l) = self.lists.last_mut() {
                    let (m, c) = if done { ("☑ ", BULLET) } else { ("☐ ", DIM) };
                    l.width = m.width();
                    l.pending = Some((m.to_string(), Style::default().fg(c)));
                }
            }
            Event::Html(h) => {
                self.flush_inline();
                let dim = Style::default().fg(DIM);
                let avail = self.content_width();
                let mut bodies: Vec<Vec<Span<'static>>> = Vec::new();
                for l in h.lines() {
                    for chunk in split_by_width(&[(l.to_string(), dim)], avail) {
                        bodies.push(chunk.into_iter().map(|(t, st)| Span::styled(t, st)).collect());
                    }
                }
                self.push_prefixed(bodies);
            }
            Event::InlineHtml(h) => {
                self.inline.push(Piece::Text(h.to_string(), Style::default().fg(DIM)));
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.push_blank();
                self.heading = Some(level);
                let color = match level {
                    HeadingLevel::H1 => H1,
                    HeadingLevel::H2 => H2,
                    HeadingLevel::H3 => H3,
                    _ => H4,
                };
                self.styles
                    .push(Style::default().fg(color).add_modifier(Modifier::BOLD));
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.quote_depth += 1;
                self.styles
                    .push(Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some(lang);
                self.code_buf.clear();
            }
            Tag::List(start) => {
                self.flush_inline();
                self.lists.push(ListLevel { ordered: start, width: 2, pending: None });
            }
            Tag::Item => {
                self.flush_inline();
                let depth = self.lists.len();
                if let Some(l) = self.lists.last_mut() {
                    let marker = match l.ordered {
                        Some(n) => {
                            l.ordered = Some(n + 1);
                            format!("{n}. ")
                        }
                        None => {
                            let b = match depth {
                                1 => "• ",
                                2 => "◦ ",
                                _ => "▪ ",
                            };
                            b.to_string()
                        }
                    };
                    l.width = marker.width();
                    l.pending = Some((marker, Style::default().fg(BULLET)));
                }
            }
            Tag::Emphasis => self.styles.push(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.styles
                    .push(Style::default().fg(LINK).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { .. } => {
                self.styles.push(Style::default().add_modifier(Modifier::ITALIC));
                self.inline
                    .push(Piece::Text("[imagem: ".into(), Style::default().fg(DIM)));
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.table = Some(TableState {
                    aligns,
                    rows: Vec::new(),
                    cur: Vec::new(),
                    header: false,
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = &mut self.table {
                    t.in_head = true;
                    t.cur.clear();
                }
            }
            Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.cur.clear();
                }
            }
            Tag::TableCell => {
                if self.table.as_ref().is_some_and(|t| t.in_head) {
                    self.styles
                        .push(Style::default().fg(H2).add_modifier(Modifier::BOLD));
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline();
                if self.lists.is_empty() {
                    if self.quote_depth > 0 {
                        let bar = self.quote_prefix();
                        self.lines.push(Line::from(bar));
                    } else {
                        self.push_blank();
                    }
                }
            }
            TagEnd::Heading(_) => self.finish_heading(),
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.styles.pop();
                if self.quote_depth == 0 {
                    let only_bar = self
                        .lines
                        .last()
                        .is_some_and(|l| !l.spans.is_empty() && l.spans.iter().all(|s| s.content == "▎ "));
                    if only_bar {
                        self.lines.pop();
                    }
                    self.push_blank();
                }
            }
            TagEnd::CodeBlock => {
                self.render_code();
                self.code = None;
                if self.lists.is_empty() {
                    self.push_blank();
                }
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.push_blank();
                }
            }
            TagEnd::Item => self.flush_inline(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                if let Some(url) = self.links.pop() {
                    if !url.is_empty() && !url.starts_with('#') {
                        self.inline
                            .push(Piece::Text(format!(" ({url})"), Style::default().fg(DIM)));
                    }
                }
            }
            TagEnd::Image => {
                self.inline.push(Piece::Text("]".into(), Style::default().fg(DIM)));
                self.styles.pop();
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(t) = &mut self.table {
                    t.cur.push(cell);
                    if t.in_head {
                        self.styles.pop();
                    }
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.cur);
                    t.rows.push(row);
                    t.header = true;
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.cur);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
                self.push_blank();
            }
            _ => {}
        }
    }

    fn push_blank(&mut self) {
        match self.lines.last() {
            None => {}
            Some(l) if is_blank(l) => {}
            Some(_) => self.lines.push(Line::default()),
        }
    }

    fn quote_prefix(&self) -> Vec<Span<'static>> {
        (0..self.quote_depth)
            .map(|_| Span::styled("▎ ", Style::default().fg(Color::Rgb(63, 185, 80))))
            .collect()
    }

    /// Prefixos (primeira linha, linhas seguintes) com citação e indentação de listas.
    /// Não altera o estado: quem desenha a primeira linha chama `take_marker`.
    fn prefixes(&self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = self.quote_prefix();
        let mut cont = self.quote_prefix();
        for lvl in &self.lists {
            let pad = " ".repeat(lvl.width);
            match &lvl.pending {
                Some((m, st)) => first.push(Span::styled(m.clone(), *st)),
                None => first.push(Span::raw(pad.clone())),
            }
            cont.push(Span::raw(pad));
        }
        (first, cont)
    }

    /// Marca os marcadores de item como já impressos (saem só na primeira linha).
    /// Vale para todos os níveis: `- - x` abre dois itens antes de qualquer texto.
    fn take_marker(&mut self) {
        for lvl in &mut self.lists {
            lvl.pending = None;
        }
    }

    /// Colunas disponíveis para o conteúdo depois dos prefixos de citação e lista.
    fn content_width(&self) -> usize {
        let (first, _) = self.prefixes();
        let pw: usize = first.iter().map(|s| s.content.width()).sum();
        self.width.saturating_sub(pw).max(1)
    }

    /// Acrescenta linhas já quebradas à saída, com os prefixos de citação e lista
    /// (o marcador do item só na primeira).
    fn push_prefixed(&mut self, bodies: Vec<Vec<Span<'static>>>) {
        let (first, cont) = self.prefixes();
        self.take_marker();
        for (i, body) in bodies.into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(body);
            self.lines.push(Line::from(v));
        }
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let toks = std::mem::take(&mut self.inline);
        let (first, cont) = self.prefixes();
        self.take_marker();
        let pw: usize = first.iter().map(|s| s.content.width()).sum();
        let avail = self.width.saturating_sub(pw).max(10);
        for (i, spans) in wrap(&toks, avail).into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(spans);
            self.lines.push(Line::from(v));
        }
    }

    fn finish_heading(&mut self) {
        let level = self.heading.take().unwrap_or(HeadingLevel::H6);
        let toks = std::mem::take(&mut self.inline);
        self.styles.pop();
        self.headings.push(self.lines.len());
        let (marker, color) = match level {
            HeadingLevel::H2 => ("▌ ", H2),
            HeadingLevel::H3 => ("▎ ", H3),
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

    fn render_code(&mut self) {
        let lang = self.code.clone().unwrap_or_default();
        let code = std::mem::take(&mut self.code_buf);
        let (ss, theme) = hl();
        let syntax = if lang.is_empty() {
            ss.find_syntax_plain_text()
        } else {
            ss.find_syntax_by_token(&lang)
                .unwrap_or_else(|| ss.find_syntax_plain_text())
        };
        let mut h = HighlightLines::new(syntax, theme);
        let (first, cont) = self.prefixes();
        self.take_marker();
        let pw: usize = cont.iter().map(|s| s.content.width()).sum();
        let avail = self.width.saturating_sub(pw + 2).max(10);
        let dim = Style::default().fg(DIM);

        let header = if lang.is_empty() { "╭─".to_string() } else { format!("╭─ {lang}") };
        let mut v = first;
        v.push(Span::styled(header, dim));
        self.lines.push(Line::from(v));

        for line in LinesWithEndings::from(&code) {
            let pieces: Vec<Styled> = match h.highlight_line(line, ss) {
                Ok(regions) => regions
                    .iter()
                    .map(|(st, txt)| {
                        let fg = Color::Rgb(st.foreground.r, st.foreground.g, st.foreground.b);
                        (clean_code(txt), Style::default().fg(fg).bg(CODE_BG))
                    })
                    .filter(|(t, _)| !t.is_empty())
                    .collect(),
                Err(_) => vec![(clean_code(line), Style::default().bg(CODE_BG))],
            };
            for chunk in split_by_width(&pieces, avail) {
                let w: usize = chunk.iter().map(|(t, _)| t.width()).sum();
                let mut spans = cont.clone();
                spans.push(Span::styled("│ ", dim));
                for (t, st) in chunk {
                    spans.push(Span::styled(t, st));
                }
                spans.push(Span::styled(
                    " ".repeat(avail.saturating_sub(w)),
                    Style::default().bg(CODE_BG),
                ));
                self.lines.push(Line::from(spans));
            }
        }

        let mut v = cont;
        v.push(Span::styled("╰─", dim));
        self.lines.push(Line::from(v));
    }

    fn render_table(&mut self, t: TableState) {
        let (first, cont) = self.prefixes();
        let pw: usize = cont.iter().map(|s| s.content.width()).sum();
        let avail = self.width.saturating_sub(pw);
        let total_cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if total_cols == 0 {
            return;
        }
        self.take_marker();
        // Colunas além do que cabe (cada uma precisa de MIN_COL_WIDTH + borda) são omitidas.
        let ncols = total_cols.min((avail.saturating_sub(1) / (CELL_OVERHEAD + MIN_COL_WIDTH)).max(1));
        let hidden = total_cols - ncols;
        let mut natural = vec![1usize; ncols];
        for row in &t.rows {
            for (i, cell) in row.iter().take(ncols).enumerate() {
                let w: usize = cell.iter().map(Piece::width).sum();
                natural[i] = natural[i].max(w);
            }
        }
        let widths = compute_column_widths(&natural, avail);

        let dim = Style::default().fg(DIM);
        let border = |prefix: &[Span<'static>], l: &str, m: &str, r: &str| -> Line<'static> {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == ncols { r } else { m });
            }
            let mut v = prefix.to_vec();
            v.push(Span::styled(s, dim));
            Line::from(v)
        };

        let mut out: Vec<Line<'static>> = vec![border(&first, "┌", "┬", "┐")];
        for (ri, row) in t.rows.iter().enumerate() {
            let mut v = cont.clone();
            v.push(Span::styled("│", dim));
            for c in 0..ncols {
                let empty: Vec<Piece> = Vec::new();
                let cell = row.get(c).unwrap_or(&empty);
                let (spans, w) = fit(cell, widths[c]);
                let pad = widths[c].saturating_sub(w);
                let (lead, trail) = match t.aligns.get(c).copied().unwrap_or(Alignment::None) {
                    Alignment::Right => (pad, 0),
                    Alignment::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                v.push(Span::raw(format!(" {}", " ".repeat(lead))));
                v.extend(spans);
                v.push(Span::raw(format!("{} ", " ".repeat(trail))));
                v.push(Span::styled("│", dim));
            }
            out.push(Line::from(v));
            if ri == 0 && t.header {
                out.push(border(&cont, "├", "┼", "┤"));
            }
        }
        out.push(border(&cont, "└", "┴", "┘"));
        if hidden > 0 {
            let note = [Piece::Text(format!("… +{hidden} colunas"), dim)];
            let mut v = cont.clone();
            v.extend(fit(&note, avail).0);
            out.push(Line::from(v));
        }
        self.lines.extend(out);
    }
}

/// Largura de borda e preenchimento por coluna (`│ ` + ` `) mais a borda final.
const CELL_OVERHEAD: usize = 3;
const MIN_COL_WIDTH: usize = 3;

/// Reduz as colunas mais largas até a tabela caber em `avail`, sem passar de
/// `MIN_COL_WIDTH` por coluna. Equivale a tirar 1 coluna da mais larga por vez
/// (empates saem da última), mas em O(n log largura) em vez de O(n * largura).
fn compute_column_widths(natural: &[usize], avail: usize) -> Vec<usize> {
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
fn largest_cap_within(mut lo: usize, mut hi: usize, budget: usize, sum: impl Fn(usize) -> usize) -> usize {
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

fn clean_code(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).replace('\t', "    ")
}

/// Corta a célula para caber em `w` colunas, usando "…" quando precisa truncar.
fn fit(cell: &[Piece], w: usize) -> (Vec<Span<'static>>, usize) {
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

/// Quebra em pedaços de no máximo `avail` colunas, caractere a caractere.
fn split_by_width(pieces: &[Styled], avail: usize) -> Vec<Vec<Styled>> {
    let avail = avail.max(1);
    let mut out: Vec<Vec<Styled>> = vec![Vec::new()];
    let mut w = 0;
    for (text, style) in pieces {
        for ch in text.chars() {
            let mut cw = ch.width().unwrap_or(0);
            if w + cw > avail && w > 0 {
                out.push(Vec::new());
                w = 0;
            }
            // Caractere mais largo que a linha inteira (1 coluna): não cabe de jeito nenhum.
            let ch = if cw > avail {
                cw = REPLACEMENT.width().unwrap_or(1);
                REPLACEMENT
            } else {
                ch
            };
            let line = out.last_mut().expect("sempre há uma linha");
            match line.last_mut() {
                Some((s, st)) if *st == *style => s.push(ch),
                _ => line.push((ch.to_string(), *style)),
            }
            w += cw;
        }
    }
    out
}

enum Unit {
    Space(Style),
    Word(Vec<Styled>),
    Break,
}

/// Espaços que separam palavras; os "sem quebra" (NBSP e similares) fazem parte da palavra.
fn is_breaking_space(ch: char) -> bool {
    ch.is_whitespace() && !matches!(ch, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

fn push_run(units: &mut Vec<Unit>, text: String, is_space: bool, style: Style) {
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

fn trim_trailing(lines: &mut [Vec<Span<'static>>], cur: &mut usize, trailing: &mut bool) {
    if *trailing {
        if let Some(l) = lines.last_mut() {
            l.pop();
        }
        *cur = cur.saturating_sub(1);
        *trailing = false;
    }
}

/// Quebra de linha por palavras preservando o estilo de cada trecho.
/// Trechos colados (ex.: `**negrito**,`) formam uma única "palavra" e não se separam.
fn wrap(toks: &[Piece], avail: usize) -> Vec<Vec<Span<'static>>> {
    let avail = avail.max(1);
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

    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut cur = 0usize;
    let mut trailing = false;
    for unit in units {
        match unit {
            Unit::Break => {
                trim_trailing(&mut lines, &mut cur, &mut trailing);
                lines.push(Vec::new());
                cur = 0;
            }
            Unit::Space(st) => {
                if cur == 0 || cur + 1 > avail {
                    continue;
                }
                lines.last_mut().expect("linha").push(Span::styled(" ", st));
                cur += 1;
                trailing = true;
            }
            Unit::Word(pieces) => {
                let ww: usize = pieces.iter().map(|(t, _)| t.width()).sum();
                if cur > 0 && cur + ww > avail {
                    trim_trailing(&mut lines, &mut cur, &mut trailing);
                    lines.push(Vec::new());
                    cur = 0;
                }
                trailing = false;
                if ww <= avail {
                    let line = lines.last_mut().expect("linha");
                    for (t, st) in pieces {
                        line.push(Span::styled(t, st));
                    }
                    cur += ww;
                } else {
                    for (i, chunk) in split_by_width(&pieces, avail).into_iter().enumerate() {
                        if i > 0 {
                            lines.push(Vec::new());
                        }
                        cur = chunk.iter().map(|(t, _)| t.width()).sum();
                        let line = lines.last_mut().expect("linha");
                        for (t, st) in chunk {
                            line.push(Span::styled(t, st));
                        }
                    }
                }
            }
        }
    }
    trim_trailing(&mut lines, &mut cur, &mut trailing);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESC: char = '\x1b';

    /// Remove somente sequências SGR bem formadas (`ESC [ dígitos;… m`), as que o `to_ansi` gera.
    fn strip_sgr(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(pos) = rest.find(ESC) {
            out.push_str(&rest[..pos]);
            let after = &rest[pos + 1..];
            let sgr_len = after
                .strip_prefix('[')
                .and_then(|p| p.find(|c: char| !(c.is_ascii_digit() || c == ';')).map(|n| (p, n)))
                .filter(|(p, n)| p[*n..].starts_with('m'))
                .map(|(_, n)| n + 2);
            match sgr_len {
                Some(n) => rest = &after[n..],
                None => {
                    out.push(ESC);
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn plain_output(md: &str) -> String {
        to_ansi(&render(md, 80).lines, true)
    }

    fn assert_no_control_chars(md: &str) {
        let visible = strip_sgr(&plain_output(md));
        let bad: Vec<char> = visible.chars().filter(|c| c.is_control() && *c != '\n').collect();
        assert!(bad.is_empty(), "caracteres de controle na saída: {bad:?}");
    }

    #[test]
    fn sanitiza_escape_em_titulo() {
        assert_no_control_chars("# ti\x1b]0;pwn\x07tulo\n");
    }

    #[test]
    fn sanitiza_escape_em_texto_e_codigo_inline() {
        assert_no_control_chars("texto \x1b[2J e `code \x1b]52;c;AAAA\x07`\n");
    }

    #[test]
    fn sanitiza_escape_em_url_de_link() {
        assert_no_control_chars("[x](http://a/\x1b]0;pwn\x07b)\n");
    }

    #[test]
    fn sanitiza_escape_em_bloco_de_codigo_e_linguagem() {
        assert_no_control_chars("```rs\x1b[2J\nlet a = \"\x1b[2J\";\n```\n");
        assert_no_control_chars("    indentado \x1b[31m\n");
    }

    #[test]
    fn sanitiza_escape_em_html() {
        assert_no_control_chars("<div>\x1b[2J</div>\n\ntexto <b\x1b[2J>x</b>\n");
    }

    #[test]
    fn sanitiza_escape_vindo_de_entidade() {
        assert_no_control_chars("a &#27;[2J b &#x9b;31m\n");
    }

    #[test]
    fn sanitiza_controles_c1_e_retorno_de_carro_solto() {
        assert_no_control_chars("a\u{9b}31m b\rc\n");
    }

    #[test]
    fn largura_enorme_e_limitada_em_render() {
        let out = render("---\n\n# t\n", usize::MAX);
        let widest = out.lines.iter().map(|l| l.width()).max().unwrap_or(0);
        assert!(widest <= MAX_WIDTH, "linha com {widest} colunas");
    }

    #[test]
    fn largura_minima_e_respeitada_em_render() {
        let out = render("---\n", 0);
        assert_eq!(out.lines[0].width(), MIN_WIDTH);
    }

    /// Algoritmo original (uma unidade por iteração), usado como referência.
    fn reference_widths(natural: &[usize], avail: usize) -> Vec<usize> {
        let mut widths = natural.to_vec();
        let n = widths.len();
        let mut total: usize = widths.iter().sum::<usize>() + 3 * n + 1;
        while total > avail {
            let (idx, &mx) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            if mx <= 3 {
                break;
            }
            widths[idx] -= 1;
            total -= 1;
        }
        widths
    }

    #[test]
    fn larguras_de_coluna_iguais_ao_algoritmo_de_referencia() {
        let mut seed = 12345u64;
        let mut next = |m: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) % m) as usize
        };
        for _ in 0..2000 {
            let ncols = 1 + next(8);
            let natural: Vec<usize> = (0..ncols).map(|_| 1 + next(40)).collect();
            let avail = next(120);
            assert_eq!(
                compute_column_widths(&natural, avail),
                reference_widths(&natural, avail),
                "natural={natural:?} avail={avail}"
            );
        }
    }

    #[test]
    fn larguras_de_coluna_nao_dependem_do_tamanho_da_celula() {
        let mut natural = vec![5usize; 3000];
        natural[1500] = 3_000_000;
        let started = std::time::Instant::now();
        let widths = compute_column_widths(&natural, 100);
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
        assert_eq!(widths.len(), 3000);
        assert!(widths.iter().all(|&w| w >= 3));
    }

    #[test]
    fn tabela_larga_renderiza_em_tempo_limitado() {
        let ncols = 3000;
        let header = format!("|{}\n", " h |".repeat(ncols));
        let sep = format!("|{}\n", "---|".repeat(ncols));
        let mut cells = vec!["c".to_string(); ncols];
        cells[ncols / 2] = "x".repeat(3_000_000);
        let row = format!("| {} |\n", cells.join(" | "));
        let md = format!("{header}{sep}{row}");
        let started = std::time::Instant::now();
        let out = render(&md, 80);
        assert!(!out.lines.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    fn rendered_text(md: &str) -> Vec<String> {
        rendered_text_at(md, 80)
    }

    fn rendered_text_at(md: &str, width: usize) -> Vec<String> {
        render(md, width)
            .lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn item_iniciado_por_bloco_de_codigo_mantem_marcador() {
        let lines = rendered_text("- ```rust\n  let a = 1;\n  ```\n");
        assert!(lines[0].starts_with("• "), "primeira linha: {:?}", lines[0]);
        assert!(!lines[1].contains('•'));
    }

    #[test]
    fn item_ordenado_iniciado_por_bloco_de_codigo_mantem_numero() {
        let lines = rendered_text("1. ```\n   x\n   ```\n2. dois\n");
        assert!(lines[0].starts_with("1. "), "primeira linha: {:?}", lines[0]);
        assert!(lines.iter().any(|l| l.starts_with("2. ")));
    }

    #[test]
    fn item_iniciado_por_tabela_mantem_marcador() {
        let lines = rendered_text("- | a | b |\n  |---|---|\n  | 1 | 2 |\n");
        assert!(lines[0].starts_with("• "), "primeira linha: {:?}", lines[0]);
        assert_eq!(lines.iter().filter(|l| l.contains('•')).count(), 1);
    }

    #[test]
    fn marcador_sai_uma_unica_vez_por_item() {
        let lines = rendered_text("- texto longo que quebra em varias linhas porque e bem comprido mesmo e passa de oitenta colunas facilmente\n");
        assert_eq!(lines.iter().filter(|l| l.contains('•')).count(), 1);
    }

    #[test]
    fn preserva_texto_comum_e_tabulacao_em_codigo() {
        let out = strip_sgr(&plain_output("```\n\tx\r\ny\n```\n"));
        assert!(out.contains("    x"), "tab deveria virar espaços: {out:?}");
        assert!(out.contains('y'));
    }

    fn assert_fits(md: &str, width: usize) {
        for (i, l) in render(md, width).lines.iter().enumerate() {
            let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(l.width() <= width, "linha {i} com {} colunas (max {width}): {text:?}", l.width());
        }
    }

    #[test]
    fn regua_respeita_prefixo_de_citacao_e_lista() {
        assert_fits("> ---\n", 30);
        assert_fits("- a\n\n  ---\n", 30);
        assert_fits("> - a\n>\n>   ---\n", 30);
        let lines = rendered_text_at("> ---\n", 30);
        assert!(lines.iter().any(|l| l.starts_with("▎ ─")), "{lines:?}");
        let lines = rendered_text_at("- ---x\n\n  ---\n", 30);
        assert!(lines.iter().any(|l| l.starts_with("  ─")), "{lines:?}");
    }

    #[test]
    fn html_em_bloco_respeita_prefixo_e_largura() {
        let long = "x".repeat(200);
        assert_fits(&format!("> <div>{long}</div>\n"), 40);
        assert_fits(&format!("- item\n\n  <div>{long}</div>\n"), 40);
        assert_fits(&format!("<div>{long}</div>\n"), 40);
    }

    #[test]
    fn titulos_respeitam_prefixo_de_citacao_e_lista() {
        assert_fits("> # um titulo bem comprido que precisa quebrar em varias linhas\n", 30);
        assert_fits("- ## um titulo bem comprido que precisa quebrar em varias linhas\n", 30);
        assert_fits("> ### um titulo bem comprido que precisa quebrar em varias linhas\n", 30);
    }

    #[test]
    fn titulo_em_citacao_mantem_barra_na_primeira_linha() {
        let lines = rendered_text("> # titulo\n");
        assert!(lines.iter().any(|l| l.starts_with("▎ ") && l.contains("titulo")), "{lines:?}");
    }

    #[test]
    fn tabela_com_muitas_colunas_cabe_na_largura() {
        let n = 30;
        let header = format!("|{}\n", " h |".repeat(n));
        let sep = format!("|{}\n", "---|".repeat(n));
        let row = format!("|{}\n", " c |".repeat(n));
        assert_fits(&format!("{header}{sep}{row}"), 20);
        assert_fits(&format!("> {header}> {sep}> {row}"), 20);
    }

    #[test]
    fn tabela_com_colunas_ocultas_avisa() {
        let n = 30;
        let header = format!("|{}\n", " h |".repeat(n));
        let sep = format!("|{}\n", "---|".repeat(n));
        let lines = rendered_text_at(&format!("{header}{sep}"), 20);
        assert!(lines.iter().any(|l| l.contains("colunas")), "{lines:?}");
    }

    #[test]
    fn caractere_largo_nao_estoura_uma_coluna() {
        let pieces = vec![("世界a".to_string(), Style::default())];
        for chunk in split_by_width(&pieces, 1) {
            let w: usize = chunk.iter().map(|(t, _)| t.width()).sum();
            assert!(w <= 1, "chunk com {w} colunas: {chunk:?}");
        }
    }

    #[test]
    fn lista_dentro_de_item_mantem_marcador_externo() {
        let lines = rendered_text("- - x\n");
        assert_eq!(lines[0], "• ◦ x");
        let lines = rendered_text("1. - x\n   - y\n");
        assert!(lines[0].starts_with("1. ◦ x"), "{lines:?}");
        assert!(lines[1].starts_with("   ◦ y"), "{lines:?}");
    }

    #[test]
    fn linhas_respeitam_largura_em_documento_misto() {
        let md = "> - item com texto comprido que precisa quebrar bastante\n>   ```\n>   codigo largo largo largo largo largo\n>   ```\n\n1. - # t\n";
        for w in [20, 25, 40, 80] {
            assert_fits(md, w);
        }
    }

    #[test]
    fn entidade_de_nova_linha_nao_vira_quebra() {
        let lines = rendered_text("a&#10;b\n");
        assert_eq!(lines, vec!["a b"]);
    }

    #[test]
    fn quebra_forcada_continua_quebrando_linha() {
        let lines = rendered_text("a\\\nb\n");
        assert_eq!(lines, vec!["a", "b"]);
    }

    #[test]
    fn codigo_em_linha_preserva_espacos() {
        assert_eq!(rendered_text("x `a      b` y\n"), vec!["x a      b y"]);
    }

    #[test]
    fn codigo_em_linha_longo_respeita_largura() {
        assert_fits("`um trecho de codigo com varias palavras separadas por espacos` fim\n", 20);
    }

    #[test]
    fn espaco_sem_quebra_nao_e_separador_de_palavra() {
        assert_eq!(rendered_text("aaaaaaaaaa\u{a0}bbbbbbbbb\n"), vec!["aaaaaaaaaa\u{a0}bbbbbbbbb"]);
    }
}
