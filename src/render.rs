//! Converte Markdown em linhas estilizadas (ratatui) já quebradas na largura pedida.
//!
//! O mesmo resultado serve para os dois modos do `mdv`: a TUI desenha as linhas
//! direto no `ratatui` e o modo `--plain` converte para ANSI e imprime.

use std::sync::OnceLock;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
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

type Piece = (String, Style);

/// Resultado da renderização.
#[derive(Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    /// Índices (em `lines`) onde cada título começa.
    pub headings: Vec<usize>,
}

pub fn render(md: &str, width: usize) -> Rendered {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer {
        width: width.max(10),
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
        r.handle(ev);
    }
    r.flush_inline();
    while r.lines.last().is_some_and(is_blank) {
        r.lines.pop();
    }
    Rendered { lines: r.lines, headings: r.headings }
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
                    self.inline.push((t.to_string(), st));
                }
            }
            Event::Code(c) => {
                let st = self
                    .style()
                    .patch(Style::default().fg(INLINE_CODE_FG).bg(INLINE_CODE_BG));
                self.inline.push((c.to_string(), st));
            }
            Event::SoftBreak => {
                let st = self.style();
                self.inline.push((" ".into(), st));
            }
            Event::HardBreak => {
                self.inline.push(("\n".into(), Style::default()));
            }
            Event::Rule => {
                self.flush_inline();
                self.push_blank();
                let rule = "─".repeat(self.width);
                self.lines.push(Line::from(Span::styled(rule, Style::default().fg(DIM))));
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
                for l in h.lines() {
                    self.lines
                        .push(Line::from(Span::styled(l.to_string(), Style::default().fg(DIM))));
                }
            }
            Event::InlineHtml(h) => {
                self.inline.push((h.to_string(), Style::default().fg(DIM)));
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
                    .push(("[imagem: ".into(), Style::default().fg(DIM)));
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
                            .push((format!(" ({url})"), Style::default().fg(DIM)));
                    }
                }
            }
            TagEnd::Image => {
                self.inline.push(("]".into(), Style::default().fg(DIM)));
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
    /// Consome o marcador pendente do item atual.
    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = self.quote_prefix();
        let mut cont = self.quote_prefix();
        let n = self.lists.len();
        for (i, lvl) in self.lists.iter_mut().enumerate() {
            let pad = " ".repeat(lvl.width);
            if i + 1 == n {
                match lvl.pending.take() {
                    Some((m, st)) => first.push(Span::styled(m, st)),
                    None => first.push(Span::raw(pad.clone())),
                }
            } else {
                first.push(Span::raw(pad.clone()));
            }
            cont.push(Span::raw(pad));
        }
        (first, cont)
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let toks = std::mem::take(&mut self.inline);
        let (first, cont) = self.prefixes();
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
        let wrapped = wrap(&toks, self.width.saturating_sub(mw).max(10));
        for (i, spans) in wrapped.into_iter().enumerate() {
            let mut v: Vec<Span<'static>> = Vec::new();
            if mw > 0 {
                v.push(if i == 0 {
                    Span::styled(marker, Style::default().fg(color))
                } else {
                    Span::raw(" ".repeat(mw))
                });
            }
            v.extend(spans);
            self.lines.push(Line::from(v));
        }
        if matches!(level, HeadingLevel::H1) {
            let rule = "═".repeat(self.width);
            self.lines
                .push(Line::from(Span::styled(rule, Style::default().fg(DIM))));
        }
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
        let (_, cont) = self.prefixes();
        let pw: usize = cont.iter().map(|s| s.content.width()).sum();
        let avail = self.width.saturating_sub(pw + 2).max(10);
        let dim = Style::default().fg(DIM);

        let header = if lang.is_empty() { "╭─".to_string() } else { format!("╭─ {lang}") };
        let mut v = cont.clone();
        v.push(Span::styled(header, dim));
        self.lines.push(Line::from(v));

        for line in LinesWithEndings::from(&code) {
            let pieces: Vec<Piece> = match h.highlight_line(line, ss) {
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
        let (_, cont) = self.prefixes();
        let pw: usize = cont.iter().map(|s| s.content.width()).sum();
        let avail = self.width.saturating_sub(pw);
        let ncols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let mut widths = vec![1usize; ncols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                let w: usize = cell.iter().map(|(s, _)| s.width()).sum();
                widths[i] = widths[i].max(w);
            }
        }
        let mut total: usize = widths.iter().sum::<usize>() + 3 * ncols + 1;
        while total > avail {
            let (idx, &mx) = widths
                .iter()
                .enumerate()
                .max_by_key(|(_, w)| **w)
                .expect("ncols > 0");
            if mx <= 3 {
                break;
            }
            widths[idx] -= 1;
            total -= 1;
        }

        let dim = Style::default().fg(DIM);
        let border = |l: &str, m: &str, r: &str| -> Line<'static> {
            let mut s = String::from(l);
            for (i, w) in widths.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                s.push_str(if i + 1 == ncols { r } else { m });
            }
            let mut v = cont.clone();
            v.push(Span::styled(s, dim));
            Line::from(v)
        };

        let mut out: Vec<Line<'static>> = vec![border("┌", "┬", "┐")];
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
                out.push(border("├", "┼", "┤"));
            }
        }
        out.push(border("└", "┴", "┘"));
        self.lines.extend(out);
    }
}

fn clean_code(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).replace('\t', "    ")
}

/// Corta a célula para caber em `w` colunas, usando "…" quando precisa truncar.
fn fit(cell: &[Piece], w: usize) -> (Vec<Span<'static>>, usize) {
    let total: usize = cell.iter().map(|(s, _)| s.width()).sum();
    if total <= w {
        let spans = cell
            .iter()
            .map(|(t, st)| Span::styled(t.clone(), *st))
            .collect();
        return (spans, total);
    }
    let limit = w.saturating_sub(1);
    let mut used = 0;
    let mut out: Vec<Span<'static>> = Vec::new();
    'outer: for (text, st) in cell {
        let mut buf = String::new();
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > limit {
                if !buf.is_empty() {
                    out.push(Span::styled(buf, *st));
                }
                out.push(Span::styled("…", *st));
                used += 1;
                break 'outer;
            }
            buf.push(ch);
            used += cw;
        }
        out.push(Span::styled(buf, *st));
    }
    (out, used)
}

/// Quebra em pedaços de no máximo `avail` colunas, caractere a caractere.
fn split_by_width(pieces: &[Piece], avail: usize) -> Vec<Vec<Piece>> {
    let avail = avail.max(1);
    let mut out: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut w = 0;
    for (text, style) in pieces {
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if w + cw > avail && w > 0 {
                out.push(Vec::new());
                w = 0;
            }
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
    Word(Vec<Piece>),
    Break,
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
    for (text, style) in toks {
        if text == "\n" {
            units.push(Unit::Break);
            continue;
        }
        let mut buf = String::new();
        let mut buf_space = false;
        for ch in text.chars() {
            let sp = ch.is_whitespace();
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
