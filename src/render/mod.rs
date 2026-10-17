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

use mdv_text::summary_text;

use crate::graphics;
use crate::mem;
use crate::image::{halfblock, Graphic, ImageState, ImageStore, MAX_IMAGE_COLS, MAX_IMAGE_ROWS};

/// Limites de largura de renderização (evitam alocações gigantes em `repeat`).
pub const MIN_WIDTH: usize = 20;
pub const MAX_WIDTH: usize = 1000;

/// Menor largura útil para texto corrido e código, mesmo com prefixos largos.
const MIN_BODY_WIDTH: usize = 10;

mod ansi;
mod renderer;
mod highlight;
mod images;
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
use self::highlight::*;
use self::images::*;
use self::list::*;
use self::renderer::Renderer;
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

/// Opções de renderização: largura, se houver onde buscar imagens e a rolagem do código.
#[derive(Default)]
pub struct RenderOpts<'a> {
    pub width: usize,
    pub images: Option<&'a dyn ImageProvider>,
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

fn is_blank(l: &Line<'_>) -> bool {
    l.spans.iter().all(|s| s.content.trim().is_empty())
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
