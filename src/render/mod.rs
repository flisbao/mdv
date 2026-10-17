//! Converte Markdown em linhas estilizadas (ratatui) já quebradas na largura pedida.
//!
//! O mesmo resultado serve para os dois modos do `mdv`: a TUI desenha as linhas
//! direto no `ratatui` e o modo `--plain` converte para ANSI e imprime.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) use mdv_text::{parse_options, sanitize};

use crate::graphics;
use crate::mem;
use crate::image::{halfblock, ImageState, ImageStore, MAX_IMAGE_COLS, MAX_IMAGE_ROWS};

/// Limites de largura de renderização (evitam alocações gigantes em `repeat`).
pub const MIN_WIDTH: usize = 20;
pub const MAX_WIDTH: usize = 1000;

/// Menor largura útil para texto corrido e código, mesmo com prefixos largos.
const MIN_BODY_WIDTH: usize = 10;

/// Orçamento de realce por documento: o realce custa ~3 s/MiB e roda a cada re-render,
/// então blocos que estourem o que resta do orçamento (ou o teto de um único bloco) saem sem cor.
const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

mod ansi;
mod inline;
mod list;
mod table;
mod wrap;
mod theme;
mod model;
mod outline;

pub(crate) use self::theme::DIM;
use self::theme::*;
pub use self::model::*;
pub use self::outline::*;
pub use self::ansi::*;
use self::wrap::*;
pub use self::inline::{link_id, strip_link_marks};
use self::inline::*;
use self::list::*;
use self::table::*;

const REPLACEMENT: char = '\u{FFFD}';

/// Trecho de texto com estilo.
type Styled = (String, Style);

/// Converte offsets de byte (crescentes) em número de linha, contando `\n` de forma incremental.
#[derive(Default)]
struct LineCursor {
    pos: usize,
    line: usize,
}

impl LineCursor {
    fn line_at(&mut self, md: &str, off: usize) -> usize {
        if off < self.pos {
            *self = LineCursor::default();
        }
        let off = off.min(md.len());
        self.line += md.as_bytes()[self.pos..off].iter().filter(|&&b| b == b'\n').count();
        self.pos = off;
        self.line
    }
}

/// Início de um bloco que tem linha própria no fonte.
fn starts_source_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::List(_)
            | Tag::Item
            | Tag::Table(_)
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
    )
}

/// Teto de links marcados por documento (o índice cabe em 16 bits).
const MAX_LINKS: usize = 5000;
const LINK_MARK_R: u8 = 0xFE;

/// Blocos ```diff e ```patch recebem cores de diff em vez do realce de sintaxe.
fn is_diff_lang(lang: &str) -> bool {
    lang.eq_ignore_ascii_case("diff") || lang.eq_ignore_ascii_case("patch")
}

/// Estilo de uma linha de diff unificado, pelo primeiro caractere.
fn diff_style(line: &str) -> Style {
    let base = Style::default().bg(CODE_BG);
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
        base.fg(H4).add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        base.fg(DIFF_HUNK_FG)
    } else if line.starts_with('+') {
        Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG)
    } else if line.starts_with('-') {
        Style::default().fg(DIFF_DEL_FG).bg(DIFF_DEL_BG)
    } else {
        base.fg(QUOTE)
    }
}

/// Mostra `code` como um bloco de código com a linguagem `lang` (usado para a fonte crua).
pub fn render_code_only(code: &str, lang: &str, width: usize) -> Rendered {
    render_code_only_at(code, lang, width, None)
}

/// Como `render_code_only`; com `scroll` o código não quebra e começa na coluna `scroll`.
pub fn render_code_only_at(code: &str, lang: &str, width: usize, scroll: Option<usize>) -> Rendered {
    let mut r = Renderer::new(width, None);
    r.code_scroll = scroll;
    r.render_code(&CodeBlock { lang: lang.to_string(), buf: sanitize(code).into_owned() });
    memory_error().unwrap_or(r.out)
}

/// Desenha só uma tabela já em células de texto (CSV/TSV); `header` estiliza a primeira linha.
pub fn render_table_only(
    rows: Vec<Vec<String>>,
    header: bool,
    aligns: Vec<Alignment>,
    width: usize,
) -> Rendered {
    let head_style = Style::default().fg(H2).add_modifier(Modifier::BOLD);
    let rows: Vec<Vec<Vec<Piece>>> = rows
        .into_iter()
        .enumerate()
        .map(|(ri, row)| {
            row.into_iter()
                .map(|cell| {
                    let st = if header && ri == 0 { head_style } else { Style::default() };
                    vec![Piece::Text(sanitize(&cell).into_owned(), st)]
                })
                .collect()
        })
        .collect();
    let mut r = Renderer::new(width, None);
        r.render_table(TableBuilder { aligns, rows, cur: Vec::new(), has_header: header, in_head: false });
    memory_error().unwrap_or(r.out)
}

/// Erro de memória como documento, se o limite foi passado.
pub fn memory_error() -> Option<Rendered> {
    mem::check().err().map(|e| error_rendered(&e.to_string()))
}

/// Uma linha vermelha com a mensagem de erro (saneada).
pub fn error_rendered(msg: &str) -> Rendered {
    let st = Style::default().fg(Color::Rgb(248, 81, 73)).add_modifier(Modifier::BOLD);
    Rendered {
        lines: vec![Line::from(Span::styled(sanitize(msg).into_owned(), st))],
        ..Rendered::default()
    }
}

pub fn clamp_width(width: usize) -> usize {
    width.clamp(MIN_WIDTH, MAX_WIDTH)
}

fn summary_text(line: &str) -> Option<&str> {
    line.trim().strip_prefix("<summary>")?.strip_suffix("</summary>").map(str::trim)
}

/// Opções de renderização: largura, se houver onde buscar imagens e a rolagem do código.
#[derive(Default)]
pub struct RenderOpts<'a> {
    pub width: usize,
    pub images: Option<&'a ImageStore>,
    /// `None`: código quebrado na largura. `Some(n)`: sem quebra, a partir da coluna `n`.
    pub code_scroll: Option<usize>,
}

#[allow(dead_code)] // atalho usado pelos testes
pub fn render(md: &str, width: usize) -> Rendered {
    render_with(md, &RenderOpts { width, images: None, ..Default::default() })
}

pub fn render_with(md: &str, opts: &RenderOpts) -> Rendered {
    let parse_opts = parse_options();
    if let Err(e) = mem::check_parse(md.len()) {
        return error_rendered(&e.to_string());
    }
    let mut r = Renderer::new(opts.width, opts.images);
    r.code_scroll = opts.code_scroll;
    let mut cursor = LineCursor::default();
    for (ev, range) in Parser::new_ext(md, parse_opts).into_offset_iter() {
        // `Rule` e HTML geram linhas no próprio evento; um `Start` só vale para o que vem depois,
        // senão o texto do item pai (descarregado ao abrir uma sublista) herdaria a linha do filho.
        let before = matches!(ev, Event::Rule | Event::Html(_));
        let after = matches!(&ev, Event::Start(t) if starts_source_block(t));
        if before {
            r.src_line = cursor.line_at(md, range.start);
        }
        r.handle(sanitize_event(ev));
        r.out.line_src.resize(r.out.lines.len(), r.src_line);
        if after {
            r.src_line = cursor.line_at(md, range.start);
        }
        if let Some(err) = memory_error() {
            return err;
        }
    }
    r.finish()
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
        Event::FootnoteReference(label) => Event::FootnoteReference(sanitize_cow(label)),
        Event::Start(Tag::FootnoteDefinition(label)) => {
            Event::Start(Tag::FootnoteDefinition(sanitize_cow(label)))
        }
        Event::Start(Tag::Image { link_type, dest_url, title, id }) => Event::Start(Tag::Image {
            link_type,
            dest_url: sanitize_cow(dest_url),
            title: sanitize_cow(title),
            id: sanitize_cow(id),
        }),
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(sanitize_cow(lang))))
        }
        other => other,
    }
}

fn hl() -> &'static (SyntaxSet, Theme) {
    static HL: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    HL.get_or_init(|| {
        let ss = SyntaxSet::load_defaults_newlines();
        let mut ts = ThemeSet::load_defaults();
        let theme = ts
            .themes
            .remove("base16-ocean.dark")
            .expect("tema embutido `base16-ocean.dark` ausente no syntect");
        (ss, theme)
    })
}

fn is_blank(l: &Line<'_>) -> bool {
    l.spans.iter().all(|s| s.content.trim().is_empty())
}

/// Bloco de código cercado ou indentado, acumulado até o fim do bloco.
struct CodeBlock {
    lang: String,
    buf: String,
}

impl CodeBlock {
    fn new(kind: CodeBlockKind<'_>) -> Self {
        let lang = match kind {
            CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
            CodeBlockKind::Indented => String::new(),
        };
        CodeBlock { lang, buf: String::new() }
    }
}

/// Imagem Markdown em andamento: destino e posição em `inline` do "[imagem: " de fallback.
struct ImageCtx {
    url: String,
    mark: usize,
}

struct Renderer<'a> {
    width: usize,
    out: Rendered,
    inline: InlineBuffer,
    lists: ListStack,
    quote_depth: usize,
    code: Option<CodeBlock>,
    table: Option<TableBuilder>,
    highlight_left: usize,
    images: Option<&'a ImageStore>,
    image: Option<ImageCtx>,
    src_line: usize,
    code_scroll: Option<usize>,
    /// Imagens vistas dentro de células (alt, imagem): desenhadas depois da tabela.
    table_figures: Vec<(String, Arc<image::DynamicImage>)>,
}

impl<'a> Renderer<'a> {
    fn new(width: usize, images: Option<&'a ImageStore>) -> Self {
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

    fn finish(mut self) -> Rendered {
        self.flush_inline();
        while self.out.lines.last().is_some_and(is_blank) {
            self.out.lines.pop();
        }
        let n = self.out.lines.len();
        self.out.line_src.resize(n, self.src_line);
        self.out
    }

    fn handle(&mut self, ev: Event<'_>) {
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

    fn rule(&mut self) {
        self.flush_inline();
        self.push_blank();
        let rule = "─".repeat(self.content_width());
        self.push_prefixed(vec![vec![Span::styled(rule, Style::default().fg(DIM))]]);
        self.push_blank();
    }

    fn html_block(&mut self, html: &str) {
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

    fn start(&mut self, tag: Tag<'_>) {
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

    fn start_heading(&mut self, level: HeadingLevel) {
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

    fn end(&mut self, tag: TagEnd) {
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

    fn end_paragraph(&mut self) {
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

    fn end_blockquote(&mut self) {
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

    fn end_link(&mut self) {
        self.inline.pop_style();
        if let Some(url) = self.inline.links.pop() {
            if !url.is_empty() && !url.starts_with('#') {
                self.inline.push_dim(format!(" ({url})"));
            }
        }
    }

    fn end_image(&mut self) {
        self.inline.pop_style();
        let in_table = self.table.is_some();
        let state = self
            .image
            .take()
            .zip(self.images)
            .map(|(ctx, store)| (ctx.mark, store.get(&ctx.url)));
        match state {
            // Em célula não há como intercalar linhas de imagem: a célula ganha `↓n` e a
            // figura é desenhada depois da tabela (`flush_table_figures`).
            Some((mark, ImageState::Ready(img))) if in_table => {
                let from = (mark + 1).min(self.inline.pieces.len());
                let alt: String = self.inline.pieces[from..].iter().map(|p| p.as_text().0).collect();
                self.table_figures.push((alt, img));
                let n = self.table_figures.len();
                self.inline.push_dim(format!(" ↓{n}]"));
            }
            Some((mark, ImageState::Ready(img))) => {
                self.inline.pieces.truncate(mark);
                self.flush_inline();
                self.push_image(&img);
            }
            Some((_, ImageState::Loading)) => self.inline.push_dim(" … carregando]"),
            Some((_, ImageState::Failed(e))) => self.inline.push_dim(format!(" — {}]", sanitize(&e))),
            None => self.inline.push_dim("]"),
        }
    }

    /// Imagens que estavam em células: a tabela só tem o `↓n`; aqui vêm elas, numeradas, cada uma
    /// com uma legenda de uma linha (cortada na largura) e o desenho.
    fn flush_table_figures(&mut self) {
        let figures = std::mem::take(&mut self.table_figures);
        let dim = Style::default().fg(DIM);
        for (i, (alt, img)) in figures.into_iter().enumerate() {
            let (_, cont) = self.prefixes();
            let alt = alt.trim();
            let caption = if alt.is_empty() { format!("↓{}", i + 1) } else { format!("↓{} {alt}", i + 1) };
            let first_row = split_by_width(&[(caption, dim)], self.content_width())
                .into_iter()
                .next()
                .unwrap_or_default();
            let mut v = cont;
            v.extend(first_row.into_iter().map(|(t, st)| Span::styled(t, st)));
            self.out.lines.push(Line::from(v));
            self.push_image(&img);
        }
    }

    fn push_image(&mut self, img: &Arc<image::DynamicImage>) {
        let (first, cont) = self.prefixes();
        self.take_marker();
        let max_cols = self.width.saturating_sub(spans_width(&first)).clamp(1, MAX_IMAGE_COLS);
        if self.images.is_some_and(ImageStore::graphics_enabled) {
            let (cols, rows) = graphics::cell_size(img.width(), img.height(), max_cols, MAX_IMAGE_ROWS);
            if let Some(g) = self.images.and_then(|s| s.graphic(img, cols, rows)) {
                for (i, span) in graphics::placeholder_rows(g.id, cols, rows).into_iter().enumerate() {
                    let mut v = if i == 0 { first.clone() } else { cont.clone() };
                    v.push(span);
                    self.out.lines.push(Line::from(v));
                }
                if !self.out.graphics.iter().any(|x| x.id == g.id) {
                    self.out.graphics.push(GraphicRef { id: g.id, cols, rows, png: g.png });
                }
                return;
            }
        }
        for (i, line) in halfblock(img, max_cols, MAX_IMAGE_ROWS).into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(line.spans);
            self.out.lines.push(Line::from(v));
        }
    }

    fn end_table_cell(&mut self) {
        let cell = self.inline.take();
        let Some(t) = &mut self.table else { return };
        t.push_cell(cell);
        if t.in_head {
            self.inline.pop_style();
        }
    }

    fn push_blank(&mut self) {
        match self.out.lines.last() {
            None => {}
            Some(l) if is_blank(l) => {}
            Some(_) => self.out.lines.push(Line::default()),
        }
    }

    fn quote_prefix(&self) -> Vec<Span<'static>> {
        (0..self.quote_depth)
            .map(|_| Span::styled(QUOTE_BAR, Style::default().fg(QUOTE_BAR_COLOR)))
            .collect()
    }

    /// Prefixos (primeira linha, linhas seguintes) com citação e indentação de listas.
    /// Não altera o estado: quem desenha a primeira linha chama `take_marker`.
    fn prefixes(&self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = self.quote_prefix();
        let mut cont = self.quote_prefix();
        self.lists.extend_prefixes(&mut first, &mut cont);
        (first, cont)
    }

    fn take_marker(&mut self) {
        self.lists.take_markers();
    }

    /// Colunas disponíveis para o conteúdo depois dos prefixos de citação e lista.
    fn content_width(&self) -> usize {
        let (first, _) = self.prefixes();
        self.width.saturating_sub(spans_width(&first)).max(1)
    }

    /// Acrescenta linhas já quebradas à saída, com os prefixos de citação e lista
    /// (o marcador do item só na primeira).
    fn push_prefixed(&mut self, bodies: Vec<Vec<Span<'static>>>) {
        let (first, cont) = self.prefixes();
        self.take_marker();
        for (i, body) in bodies.into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(body);
            self.out.lines.push(Line::from(v));
        }
    }

    fn flush_inline(&mut self) {
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

    fn finish_heading(&mut self, level: HeadingLevel) {
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

    fn render_code(&mut self, block: &CodeBlock) {
        let mut label = block.lang.clone();
        if block.lang == "mermaid" {
            if let Some(store) = self.images.filter(|s| s.mermaid_enabled()) {
                match store.get_mermaid(&block.buf) {
                    ImageState::Ready(img) => {
                        let start = self.out.lines.len();
                        self.push_image(&img);
                        let key = ImageStore::mermaid_key(&block.buf);
                        self.out.mermaids.push(MermaidSpan { start, end: self.out.lines.len(), key });
                        return;
                    }
                    ImageState::Loading => label = "mermaid (renderizando…)".into(),
                    ImageState::Failed(e) => label = format!("mermaid (erro: {})", sanitize(&e)),
                }
            }
        }
        let (ss, theme) = hl();
        let syntax = if block.lang.is_empty() || block.buf.len() > self.highlight_left {
            ss.find_syntax_plain_text()
        } else {
            self.highlight_left -= block.buf.len();
            ss.find_syntax_by_token(&block.lang)
                .unwrap_or_else(|| ss.find_syntax_plain_text())
        };
        let mut h = HighlightLines::new(syntax, theme);
        let is_diff = is_diff_lang(&block.lang);
        let (first, cont) = self.prefixes();
        self.take_marker();
        let avail = self.width.saturating_sub(spans_width(&cont) + 2).max(MIN_BODY_WIDTH);
        let dim = Style::default().fg(DIM);

        let start = self.out.lines.len();
        let header = if label.is_empty() { "╭─".to_string() } else { format!("╭─ {label}") };
        let mut v = first;
        v.push(Span::styled(header, dim));
        self.out.lines.push(Line::from(v));

        for line in LinesWithEndings::from(&block.buf) {
            if mem::exceeded() {
                break;
            }
            let pieces: Vec<Styled> = if is_diff {
                vec![(clean_code(line), diff_style(line))]
            } else {
                match h.highlight_line(line, ss) {
                    Ok(regions) => regions
                        .iter()
                        .map(|(st, txt)| {
                            let fg = Color::Rgb(st.foreground.r, st.foreground.g, st.foreground.b);
                            (clean_code(txt), Style::default().fg(fg).bg(CODE_BG))
                        })
                        .filter(|(t, _)| !t.is_empty())
                        .collect(),
                    Err(_) => vec![(clean_code(line), Style::default().bg(CODE_BG))],
                }
            };
            let line_width: usize = pieces.iter().map(|(t, _)| t.width()).sum();
            self.out.code_overflow = self.out.code_overflow.max(line_width.saturating_sub(avail));
            let chunks = match self.code_scroll {
                Some(skip) => vec![window_by_width(&pieces, skip, avail)],
                None => split_by_width(&pieces, avail),
            };
            for chunk in chunks {
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
                self.out.lines.push(Line::from(spans));
            }
        }

        let mut v = cont;
        v.push(Span::styled("╰─", dim));
        self.out.lines.push(Line::from(v));
        self.out.code_blocks.push(CodeSpan {
            start,
            end: self.out.lines.len(),
            lang: block.lang.clone(),
            text: block.buf.clone(),
        });
    }

        fn render_table(&mut self, t: TableBuilder) {
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

/// Largura de borda e preenchimento por coluna (`│ ` + ` `) mais a borda final.
const CELL_OVERHEAD: usize = 3;
const MIN_COL_WIDTH: usize = 3;

/// Teto de linhas por célula de tabela; o que passa disso termina em "…".
const MAX_CELL_LINES: usize = 8;

#[cfg(test)]
mod testutil;
#[cfg(test)]
mod tests;
