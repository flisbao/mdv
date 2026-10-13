//! Converte Markdown em linhas estilizadas (ratatui) já quebradas na largura pedida.
//!
//! O mesmo resultado serve para os dois modos do `mdv`: a TUI desenha as linhas
//! direto no `ratatui` e o modo `--plain` converte para ANSI e imprime.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::OnceLock;

use pulldown_cmark::{Alignment, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::image::{halfblock, ImageState, ImageStore, MAX_IMAGE_COLS, MAX_IMAGE_ROWS};

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
const QUOTE_BAR_COLOR: Color = Color::Rgb(63, 185, 80);

/// Barra que prefixa cada linha de uma citação.
const QUOTE_BAR: &str = "▎ ";

/// Limites de largura de renderização (evitam alocações gigantes em `repeat`).
pub const MIN_WIDTH: usize = 20;
pub const MAX_WIDTH: usize = 1000;

/// Menor largura útil para texto corrido e código, mesmo com prefixos largos.
const MIN_BODY_WIDTH: usize = 10;

/// Orçamento de realce por documento: o realce custa ~3 s/MiB e roda a cada re-render,
/// então blocos que estourem o que resta do orçamento (ou o teto de um único bloco) saem sem cor.
const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

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

/// Opções de renderização: largura e, se houver, onde buscar imagens.
pub struct RenderOpts<'a> {
    pub width: usize,
    pub images: Option<&'a ImageStore>,
}

#[allow(dead_code)] // atalho usado pelos testes
pub fn render(md: &str, width: usize) -> Rendered {
    render_with(md, &RenderOpts { width, images: None })
}

pub fn render_with(md: &str, opts: &RenderOpts) -> Rendered {
    let parse_opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer::new(opts.width, opts.images);
    for ev in Parser::new_ext(md, parse_opts) {
        r.handle(sanitize_event(ev));
    }
    r.finish()
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
            let opened = color && write_sgr(&mut out, &span.style);
            out.push_str(&span.content);
            if opened {
                out.push_str("\x1b[0m");
            }
        }
        out.push('\n');
    }
    out
}

/// Abre a sequência SGR do estilo em `out`; devolve `false` (sem escrever nada) se o estilo é vazio.
fn write_sgr(out: &mut String, st: &Style) -> bool {
    const MODIFIERS: [(Modifier, &str); 5] = [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::CROSSED_OUT, "9"),
    ];
    let start = out.len();
    out.push_str("\x1b[");
    let params_start = out.len();
    let param = |out: &mut String, args: std::fmt::Arguments<'_>| {
        if out.len() > params_start {
            out.push(';');
        }
        // Escrever em `String` não falha.
        let _ = out.write_fmt(args);
    };
    for (modifier, code) in MODIFIERS {
        if st.add_modifier.contains(modifier) {
            param(out, format_args!("{code}"));
        }
    }
    for (color, base) in [(st.fg, 38), (st.bg, 48)] {
        match color {
            Some(Color::Rgb(r, g, b)) => param(out, format_args!("{base};2;{r};{g};{b}")),
            Some(Color::Indexed(i)) => param(out, format_args!("{base};5;{i}")),
            _ => {}
        }
    }
    if out.len() == params_start {
        out.truncate(start);
        return false;
    }
    out.push('m');
    true
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

/// Marcador de item ainda não impresso e seu estilo.
type Marker = (String, Style);

struct ListLevel {
    ordered: Option<u64>,
    /// Largura do marcador do item atual (usada para indentar o conteúdo).
    width: usize,
    /// Marcador ainda não impresso (sai junto com a primeira linha do item).
    pending: Option<Marker>,
}

/// Pilha de listas abertas (a mais interna é a última).
#[derive(Default)]
struct ListStack {
    levels: Vec<ListLevel>,
}

impl ListStack {
    fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    fn open(&mut self, start: Option<u64>) {
        self.levels.push(ListLevel { ordered: start, width: 2, pending: None });
    }

    fn close(&mut self) {
        self.levels.pop();
    }

    /// Começa um item na lista mais interna, com número ou bolinha conforme o nível.
    fn start_item(&mut self) {
        let depth = self.levels.len();
        let Some(level) = self.levels.last_mut() else { return };
        let marker = match level.ordered {
            Some(n) => {
                level.ordered = Some(n.saturating_add(1));
                format!("{n}. ")
            }
            None => match depth {
                1 => "• ",
                2 => "◦ ",
                _ => "▪ ",
            }
            .to_string(),
        };
        level.set_marker(marker, BULLET);
    }

    fn set_task_marker(&mut self, done: bool) {
        let Some(level) = self.levels.last_mut() else { return };
        let (marker, color) = if done { ("☑ ", BULLET) } else { ("☐ ", DIM) };
        level.set_marker(marker.to_string(), color);
    }

    /// Marca os marcadores de item como já impressos (saem só na primeira linha).
    /// Vale para todos os níveis: `- - x` abre dois itens antes de qualquer texto.
    fn take_markers(&mut self) {
        for level in &mut self.levels {
            level.pending = None;
        }
    }

    /// Acrescenta a indentação (e os marcadores pendentes, só na primeira linha).
    fn extend_prefixes(&self, first: &mut Vec<Span<'static>>, cont: &mut Vec<Span<'static>>) {
        for level in &self.levels {
            let pad = " ".repeat(level.width);
            match &level.pending {
                Some((marker, style)) => first.push(Span::styled(marker.clone(), *style)),
                None => first.push(Span::raw(pad.clone())),
            }
            cont.push(Span::raw(pad));
        }
    }
}

impl ListLevel {
    fn set_marker(&mut self, marker: String, color: Color) {
        self.width = marker.width();
        self.pending = Some((marker, Style::default().fg(color)));
    }
}

/// Texto em linha ainda não quebrado, com as pilhas de estilo e de destinos de link.
#[derive(Default)]
struct InlineBuffer {
    pieces: Vec<Piece>,
    styles: Vec<Style>,
    links: Vec<String>,
}

impl InlineBuffer {
    fn style(&self) -> Style {
        self.styles.iter().fold(Style::default(), |a, s| a.patch(*s))
    }

    fn push_style(&mut self, style: Style) {
        self.styles.push(style);
    }

    fn pop_style(&mut self) {
        self.styles.pop();
    }

    fn push(&mut self, piece: Piece) {
        self.pieces.push(piece);
    }

    fn push_text(&mut self, text: &str) {
        let style = self.style();
        self.pieces.push(Piece::Text(text.to_string(), style));
    }

    fn push_dim(&mut self, text: impl Into<String>) {
        self.pieces.push(Piece::Text(text.into(), Style::default().fg(DIM)));
    }

    fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    fn take(&mut self) -> Vec<Piece> {
        std::mem::take(&mut self.pieces)
    }
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

/// Tabela em construção: linhas completas mais a linha em andamento.
struct TableBuilder {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Piece>>>,
    cur: Vec<Vec<Piece>>,
    has_header: bool,
    in_head: bool,
}

impl TableBuilder {
    fn new(aligns: Vec<Alignment>) -> Self {
        TableBuilder { aligns, rows: Vec::new(), cur: Vec::new(), has_header: false, in_head: false }
    }

    fn begin_head(&mut self) {
        self.in_head = true;
        self.cur.clear();
    }

    fn begin_row(&mut self) {
        self.cur.clear();
    }

    fn push_cell(&mut self, cell: Vec<Piece>) {
        self.cur.push(cell);
    }

    fn end_row(&mut self) {
        let row = std::mem::take(&mut self.cur);
        self.rows.push(row);
    }

    fn end_head(&mut self) {
        self.end_row();
        self.has_header = true;
        self.in_head = false;
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
}

impl<'a> Renderer<'a> {
    fn new(width: usize, images: Option<&'a ImageStore>) -> Self {
        Renderer {
            images,
            image: None,
            width: clamp_width(width),
            out: Rendered::default(),
            inline: InlineBuffer::default(),
            lists: ListStack::default(),
            quote_depth: 0,
            code: None,
            table: None,
            highlight_left: MAX_HIGHLIGHT_BYTES,
        }
    }

    fn finish(mut self) -> Rendered {
        self.flush_inline();
        while self.out.lines.last().is_some_and(is_blank) {
            self.out.lines.pop();
        }
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
            Event::TaskListMarker(done) => self.lists.set_task_marker(done),
            Event::Html(h) => self.html_block(&h),
            Event::InlineHtml(h) => self.inline.push_dim(h.to_string()),
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
        for l in html.lines() {
            for chunk in split_by_width(&[(l.to_string(), dim)], avail) {
                bodies.push(chunk.into_iter().map(|(t, st)| Span::styled(t, st)).collect());
            }
        }
        self.push_prefixed(bodies);
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.quote_depth += 1;
                self.inline
                    .push_style(Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC));
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
                self.inline
                    .push_style(Style::default().fg(LINK).add_modifier(Modifier::UNDERLINED));
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
            TagEnd::CodeBlock => {
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
            TagEnd::Item => self.flush_inline(),
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
                    self.render_table(&t);
                }
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
        // Em célula de tabela não há como intercalar linhas de imagem: fica o texto alternativo.
        let store = if self.table.is_some() { None } else { self.images };
        let state = self
            .image
            .take()
            .zip(store)
            .map(|(ctx, store)| (ctx.mark, store.get(&ctx.url)));
        match state {
            Some((mark, ImageState::Ready(img))) => {
                self.inline.pieces.truncate(mark);
                self.flush_inline();
                self.push_image(&img);
            }
            Some((_, ImageState::Loading)) => self.inline.push_dim(" … carregando]"),
            Some((_, ImageState::Failed(e))) => self.inline.push_dim(format!(" — {e}]")),
            None => self.inline.push_dim("]"),
        }
    }

    fn push_image(&mut self, img: &image::DynamicImage) {
        let (first, cont) = self.prefixes();
        self.take_marker();
        let cols = self.width.saturating_sub(spans_width(&first)).clamp(1, MAX_IMAGE_COLS);
        for (i, line) in halfblock(img, cols, MAX_IMAGE_ROWS).into_iter().enumerate() {
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
                        self.push_image(&img);
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
        let (first, cont) = self.prefixes();
        self.take_marker();
        let avail = self.width.saturating_sub(spans_width(&cont) + 2).max(MIN_BODY_WIDTH);
        let dim = Style::default().fg(DIM);

        let header = if label.is_empty() { "╭─".to_string() } else { format!("╭─ {label}") };
        let mut v = first;
        v.push(Span::styled(header, dim));
        self.out.lines.push(Line::from(v));

        for line in LinesWithEndings::from(&block.buf) {
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
                self.out.lines.push(Line::from(spans));
            }
        }

        let mut v = cont;
        v.push(Span::styled("╰─", dim));
        self.out.lines.push(Line::from(v));
    }

    fn render_table(&mut self, t: &TableBuilder) {
        let (first, cont) = self.prefixes();
        let avail = self.width.saturating_sub(spans_width(&cont));
        let total_cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        if total_cols == 0 {
            return;
        }
        self.take_marker();
        // Colunas além do que cabe (cada uma precisa de MIN_COL_WIDTH + borda) são omitidas.
        let ncols = total_cols.min((avail.saturating_sub(1) / (CELL_OVERHEAD + MIN_COL_WIDTH)).max(1));
        let hidden = total_cols - ncols;
        let widths = compute_column_widths(&natural_widths(&t.rows, ncols), avail);

        let mut out: Vec<Line<'static>> = vec![table_border(&first, &widths, ["┌", "┬", "┐"])];
        for (ri, row) in t.rows.iter().enumerate() {
            out.push(draw_table_row(&cont, row, &widths, &t.aligns));
            if ri == 0 && t.has_header {
                out.push(table_border(&cont, &widths, ["├", "┼", "┤"]));
            }
        }
        out.push(table_border(&cont, &widths, ["└", "┴", "┘"]));
        if hidden > 0 {
            let note = [Piece::Text(format!("… +{hidden} colunas"), Style::default().fg(DIM))];
            let mut v = cont;
            v.extend(fit(&note, avail).0);
            out.push(Line::from(v));
        }
        self.out.lines.extend(out);
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Maior largura de conteúdo de cada uma das `ncols` primeiras colunas (mínimo 1).
fn natural_widths(rows: &[Vec<Vec<Piece>>], ncols: usize) -> Vec<usize> {
    let mut natural = vec![1usize; ncols];
    for row in rows {
        for (i, cell) in row.iter().take(ncols).enumerate() {
            let w: usize = cell.iter().map(Piece::width).sum();
            natural[i] = natural[i].max(w);
        }
    }
    natural
}

/// Linha de borda da tabela; `corners` são o canto esquerdo, o cruzamento e o canto direito.
fn table_border(prefix: &[Span<'static>], widths: &[usize], corners: [&str; 3]) -> Line<'static> {
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

/// Linha de dados da tabela com cada célula truncada e alinhada à sua coluna.
fn draw_table_row(
    prefix: &[Span<'static>],
    row: &[Vec<Piece>],
    widths: &[usize],
    aligns: &[Alignment],
) -> Line<'static> {
    let dim = Style::default().fg(DIM);
    let mut v = prefix.to_vec();
    v.push(Span::styled("│", dim));
    for (c, &width) in widths.iter().enumerate() {
        let cell = row.get(c).map_or(&[][..], Vec::as_slice);
        let (spans, w) = fit(cell, width);
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

/// Monta linhas quebradas a partir de espaços e palavras.
struct LineBuilder {
    avail: usize,
    done: Vec<Vec<Span<'static>>>,
    line: Vec<Span<'static>>,
    /// Colunas já usadas na linha atual.
    cur: usize,
    /// A linha atual termina num espaço que ainda pode ser descartado.
    trailing: bool,
}

impl LineBuilder {
    fn new(avail: usize) -> Self {
        LineBuilder { avail: avail.max(1), done: Vec::new(), line: Vec::new(), cur: 0, trailing: false }
    }

    fn trim_trailing(&mut self) {
        if self.trailing {
            self.line.pop();
            self.cur = self.cur.saturating_sub(1);
            self.trailing = false;
        }
    }

    fn new_line(&mut self) {
        self.done.push(std::mem::take(&mut self.line));
        self.cur = 0;
    }

    fn break_line(&mut self) {
        self.trim_trailing();
        self.new_line();
    }

    fn space(&mut self, style: Style) {
        if self.cur == 0 || self.cur + 1 > self.avail {
            return;
        }
        self.line.push(Span::styled(" ", style));
        self.cur += 1;
        self.trailing = true;
    }

    fn word(&mut self, pieces: Vec<Styled>) {
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

    fn finish(mut self) -> Vec<Vec<Span<'static>>> {
        self.trim_trailing();
        self.done.push(self.line);
        self.done
    }
}

/// Quebra de linha por palavras preservando o estilo de cada trecho.
/// Trechos colados (ex.: `**negrito**,`) formam uma única "palavra" e não se separam.
fn wrap(toks: &[Piece], avail: usize) -> Vec<Vec<Span<'static>>> {
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

    fn distinct_code_colors(code: &str) -> usize {
        let md = format!("```rust\n{code}```\n");
        let mut colors: Vec<Option<Color>> = Vec::new();
        for span in render(&md, 80).lines.iter().flat_map(|l| &l.spans) {
            if span.style.bg == Some(CODE_BG) && span.style.fg.is_some() && !colors.contains(&span.style.fg) {
                colors.push(span.style.fg);
            }
        }
        colors.len()
    }

    #[test]
    fn codigo_pequeno_e_destacado() {
        assert!(distinct_code_colors("fn main() { let a = 1; } // x\n") > 1);
    }

    #[test]
    fn codigo_enorme_nao_e_destacado_para_nao_travar_a_ui() {
        let line = "fn main() { let a = 1; } // x\n";
        let code = line.repeat(MAX_HIGHLIGHT_BYTES / line.len() + 1);
        assert_eq!(distinct_code_colors(&code), 1);
    }

    #[test]
    fn orcamento_de_realce_vale_para_o_documento_inteiro() {
        let block = "fn main() { let a = 1; } // x\n".repeat(2000);
        let blocks = MAX_HIGHLIGHT_BYTES / block.len() + 2;
        let md = format!("```rust\n{block}```\n\n").repeat(blocks);
        let mut colors_per_block: Vec<std::collections::HashSet<String>> = Vec::new();
        for line in &render(&md, 80).lines {
            if line.spans.first().is_some_and(|s| s.content.starts_with('╭')) {
                colors_per_block.push(std::collections::HashSet::new());
            }
            for span in line.spans.iter().filter(|s| s.style.bg == Some(CODE_BG)) {
                if let (Some(set), Some(fg)) = (colors_per_block.last_mut(), span.style.fg) {
                    set.insert(format!("{fg:?}"));
                }
            }
        }
        assert_eq!(colors_per_block.len(), blocks);
        assert!(colors_per_block[0].len() > 1, "primeiro bloco cabe no orçamento");
        assert_eq!(colors_per_block[blocks - 1].len(), 1, "último bloco estoura o orçamento");
    }

    fn joined_text(r: &Rendered) -> String {
        r.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn imagem_local_vira_meio_bloco() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        std::fs::write(dir.path().join("a.png"), png.into_inner()).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("antes\n\n![logo](a.png)\n\ndepois", &RenderOpts { width: 40, images: Some(&store) });
        let t = joined_text(&r);
        assert!(t.contains('▀'), "{t}");
        assert!(!t.contains("[imagem"), "{t}");
        assert!(t.contains("antes") && t.contains("depois"));
    }

    #[test]
    fn imagem_quebrada_mostra_alt_e_motivo() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("![logo](nao.png)", &RenderOpts { width: 40, images: Some(&store) });
        let t = joined_text(&r);
        assert!(t.contains("[imagem: logo"), "{t}");
        assert!(t.contains('—'), "{t}");
    }

    #[test]
    fn imagem_em_celula_de_tabela_continua_texto() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        std::fs::write(dir.path().join("a.png"), png.into_inner()).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("| a |\n|---|\n| ![x](a.png) |\n", &RenderOpts { width: 40, images: Some(&store) });
        let t = joined_text(&r);
        assert!(!t.contains('▀'), "{t}");
        assert!(t.contains("[imagem: x]"), "{t}");
    }

    #[cfg(unix)]
    #[test]
    fn bloco_mermaid_vira_imagem_quando_habilitado() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("f.png");
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])));
        img.save(&png).unwrap();
        let bin = dir.path().join("mmdc");
        std::fs::write(&bin, format!("#!/bin/sh\ncp {} \"$4\"\n", png.display())).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        let store = ImageStore::new(None).with_mermaid(bin.to_string_lossy().into_owned(), true);
        let r = render_with("```mermaid\ngraph TD; A-->B\n```", &RenderOpts { width: 40, images: Some(&store) });
        assert!(joined_text(&r).contains('▀'));
    }

    #[test]
    fn mermaid_sem_flag_mostra_codigo() {
        let t = joined_text(&render("```mermaid\ngraph TD; A-->B\n```", 40));
        assert!(t.contains("mermaid") && t.contains("A-->B"), "{t}");
    }

    #[test]
    fn mermaid_com_falha_mostra_codigo_e_motivo() {
        let store = ImageStore::new(None).with_mermaid("/nao/existe/mmdc".into(), true);
        let r = render_with("```mermaid\ngraph TD; A-->B\n```", &RenderOpts { width: 60, images: Some(&store) });
        let t = joined_text(&r);
        assert!(t.contains("A-->B") && t.contains("erro"), "{t}");
    }

    #[test]
    fn sem_store_mantem_comportamento_antigo() {
        let t = joined_text(&render("![logo](a.png)", 40));
        assert_eq!(t.trim(), "[imagem: logo]");
    }
}
