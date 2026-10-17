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

const H1: Color = Color::Rgb(210, 168, 255);
const H2: Color = Color::Rgb(121, 192, 255);
const H3: Color = Color::Rgb(126, 231, 135);
const H4: Color = Color::Rgb(230, 237, 243);
pub(crate) const DIM: Color = Color::Rgb(110, 118, 129);
const LINK: Color = Color::Rgb(88, 166, 255);
const INLINE_CODE_FG: Color = Color::Rgb(255, 166, 87);
const INLINE_CODE_BG: Color = Color::Rgb(47, 52, 60);
const CODE_BG: Color = Color::Rgb(40, 44, 52);
const DIFF_ADD_FG: Color = Color::Rgb(126, 231, 135);
const DIFF_ADD_BG: Color = Color::Rgb(35, 64, 45);
const DIFF_DEL_FG: Color = Color::Rgb(255, 123, 114);
const DIFF_DEL_BG: Color = Color::Rgb(74, 38, 42);
const DIFF_HUNK_FG: Color = Color::Rgb(121, 192, 255);
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
    /// Destino de cada link do documento, na ordem em que aparecem; o texto do link carrega o
    /// índice na cor do sublinhado (`link_id`). A TUI tira a marca antes de desenhar.
    pub links: Vec<String>,
    /// Índices (em `lines`) onde cada título começa.
    pub headings: Vec<usize>,
    /// Títulos com nível e texto, para o `--toc`.
    pub outline: Vec<OutlineEntry>,
        /// Tabelas desenhadas, em ordem, com o intervalo de linhas de cada uma (zoom `t`).
        pub tables: Vec<TableSpan>,
    /// Linha (0-based) do fonte onde cada linha renderizada começa. Vazio fora do Markdown.
    pub line_src: Vec<usize>,
    /// Blocos de código desenhados, em ordem.
    pub code_blocks: Vec<CodeSpan>,
    /// Itens de lista de tarefas, em ordem.
    pub tasks: Vec<TaskItem>,
    /// Maior quanto uma linha de código passa das colunas disponíveis (0 se nenhuma passa).
    /// Limite do deslocamento horizontal.
    pub code_overflow: usize,
    /// Imagens gráficas usadas, sem repetir `id`. Vazio sem `--graphics`.
    pub graphics: Vec<GraphicRef>,
    /// Diagramas Mermaid desenhados como imagem, em ordem (alvo do zoom de imagem).
    pub mermaids: Vec<MermaidSpan>,
}

/// Diagrama Mermaid desenhado como imagem: linhas renderizadas `[start, end)` e a chave do PNG
/// original em `ImageStore::mermaid_png`.
#[derive(Clone)]
pub struct MermaidSpan {
    pub start: usize,
    pub end: usize,
    pub key: String,
}

/// Bloco de código desenhado: linhas renderizadas `[start, end)` e o texto cru (já saneado).
#[derive(Clone)]
pub struct CodeSpan {
    pub start: usize,
    pub end: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub lang: String,
    pub text: String,
}

/// Imagem que o terminal precisa receber (protocolo gráfico) para os placeholders aparecerem.
#[derive(Clone)]
pub struct GraphicRef {
    pub id: u32,
    pub cols: usize,
    pub rows: usize,
    pub png: Arc<Vec<u8>>,
}

/// Item de lista de tarefas e a linha renderizada onde ele começa.
#[derive(Clone, Copy)]
pub struct TaskItem {
    pub line: usize,
    pub done: bool,
}

/// Linhas renderizadas `[início, fim)` da seção `idx`: do título até o próximo de nível
/// igual ou menor, ou até `total`.
pub fn section_range(outline: &[OutlineEntry], idx: usize, total: usize) -> (usize, usize) {
    let e = &outline[idx];
    let end = outline[idx + 1..].iter().find(|n| n.level <= e.level).map_or(total, |n| n.line);
    (e.line, end)
}

/// Tarefas feitas e total num trecho do documento.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TaskCount {
    pub done: usize,
    pub total: usize,
}

/// Conta as tarefas cujas linhas estão em `[from, to)`.
pub fn count_tasks(tasks: &[TaskItem], from: usize, to: usize) -> TaskCount {
    let mut c = TaskCount::default();
    for t in tasks.iter().filter(|t| t.line >= from && t.line < to) {
        c.total += 1;
        if t.done {
            c.done += 1;
        }
    }
    c
}

/// Tarefas da seção `idx`, contando as subseções.
pub fn section_tasks(r: &Rendered, idx: usize) -> TaskCount {
    let (start, end) = section_range(&r.outline, idx, r.lines.len());
    count_tasks(&r.tasks, start, end)
}

/// Título mais profundo que começa em `line` ou antes (a seção que contém a linha).
pub fn section_at(outline: &[OutlineEntry], line: usize) -> Option<usize> {
    outline.iter().rposition(|e| e.line <= line)
}

/// Markdown cru da seção `idx` (com o título), fatiado do fonte pelo mapa `line_src`.
pub fn section_source(md: &str, r: &Rendered, idx: usize) -> Option<String> {
    if idx >= r.outline.len() || r.line_src.len() != r.lines.len() {
        return None;
    }
    let (start, end) = section_range(&r.outline, idx, r.lines.len());
    let first = *r.line_src.get(start)?;
    let last = r.line_src.get(end).copied().unwrap_or_else(|| md.lines().count());
    let body: Vec<&str> = md.lines().skip(first).take(last.saturating_sub(first)).collect();
    let mut text = body.join("\n").trim_end().to_string();
    text.push('\n');
    Some(text)
}

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

#[allow(dead_code)] // `line` será usado pela navegação do sumário
pub struct OutlineEntry {
    pub line: usize,
    pub level: u8,
    pub title: String,
}

/// Tabela guardada para ser desenhada de novo em outra largura (zoom).
#[derive(Clone)]
pub struct TableData {
    rows: Vec<Vec<Vec<Piece>>>,
    aligns: Vec<Alignment>,
    has_header: bool,
}

/// Onde uma tabela está em `Rendered.lines` (`start..end`) e como redesenhá-la.
pub struct TableSpan {
    pub start: usize,
    pub end: usize,
    pub data: TableData,
}

/// Tabela desenhada numa largura: as linhas, quantas ficam fixas no topo e quais colunas aparecem.
pub struct TableView {
    pub lines: Vec<Line<'static>>,
    /// Linhas fixas no topo (borda, cabeçalho e separador; só a borda sem cabeçalho).
    pub pinned: usize,
    /// Índice (0-based) da primeira coluna rolável mostrada, já limitado ao intervalo válido.
    pub first_col: usize,
    /// Colunas desenhadas, contando a primeira (fixa).
    pub shown_cols: usize,
    pub total_cols: usize,
}

impl TableData {
    pub fn total_cols(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Desenha a tabela em `width` colunas, sem recuo. A coluna 0 fica sempre;
    /// as demais começam em `first_col`.
    pub fn draw(&self, width: usize, first_col: usize) -> TableView {
        layout_table(self, &[], &[], width, first_col)
    }
}

/// Índice dos títulos, um por linha, indentado conforme o nível.
pub fn toc(r: &Rendered) -> String {
    let mut out = String::new();
    for (i, e) in r.outline.iter().enumerate() {
        let _ = write!(out, "{}{}", "  ".repeat(usize::from(e.level.saturating_sub(1))), e.title);
        let c = section_tasks(r, i);
        if c.total > 0 {
            let _ = write!(out, " [{}/{}]", c.done, c.total);
        }
        out.push('\n');
    }
    out
}

/// Teto de links marcados por documento (o índice cabe em 16 bits).
const MAX_LINKS: usize = 5000;
const LINK_MARK_R: u8 = 0xFE;

fn link_mark(id: usize) -> Color {
    let id = u16::try_from(id).unwrap_or(u16::MAX);
    Color::Rgb(LINK_MARK_R, (id >> 8) as u8, (id & 0xFF) as u8)
}

/// Índice (em `Rendered.links`) do link a que o estilo pertence, se for texto de link.
pub fn link_id(style: &Style) -> Option<usize> {
    match style.underline_color {
        Some(Color::Rgb(LINK_MARK_R, hi, lo)) => Some((usize::from(hi) << 8) | usize::from(lo)),
        _ => None,
    }
}

/// Tira a marca de link da linha (a marca só serve à navegação, não deve chegar ao terminal).
pub fn strip_link_marks(line: &mut Line<'static>) {
    for sp in &mut line.spans {
        if link_id(&sp.style).is_some() {
            sp.style.underline_color = None;
        }
    }
}

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

/// Desenha a tabela em `avail` colunas. A coluna 0 fica sempre; as colunas roláveis mostradas
/// começam em `first_col` (limitado para a última coluna caber no máximo). Colunas que não
/// cabem (cada uma precisa de `MIN_COL_WIDTH` mais a borda) ficam de fora e são contadas no aviso.
fn layout_table(
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

    let mut lines: Vec<Line<'static>> = vec![table_border(first, &widths, ["┌", "┬", "┐"])];
    let mut pinned = 1;
    for (ri, row) in t.rows.iter().enumerate() {
        if mem::exceeded() {
            break;
        }
        let cells: Vec<&[Piece]> =
            idx.iter().map(|&c| row.get(c).map_or(&[][..], Vec::as_slice)).collect();
        let drawn = draw_table_row(cont, &cells, &widths, &aligns);
        let is_head = ri == 0 && t.has_header;
        if is_head {
            pinned += drawn.len() + 1;
        }
        lines.extend(drawn);
        if is_head {
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

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Maior largura de conteúdo de cada coluna de `idx` (mínimo 1).
fn natural_widths(rows: &[Vec<Vec<Piece>>], idx: &[usize]) -> Vec<usize> {
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

/// Linhas de dados da tabela: cada célula é quebrada na largura da sua coluna e alinhada;
/// a linha da tabela tem a altura da maior célula.
fn draw_table_row(
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

/// Teto de linhas por célula de tabela; o que passa disso termina em "…".
const MAX_CELL_LINES: usize = 8;

/// Mesma natureza de `piece` (texto ou código) com outro conteúdo; `Break` vira espaço.
fn with_text(piece: &Piece, text: String) -> Piece {
    match piece {
        Piece::Code(_, st) => Piece::Code(text, *st),
        Piece::Text(_, st) => Piece::Text(text, *st),
        Piece::Break => Piece::Text(text, Style::default()),
    }
}

/// Mantém só os primeiros `budget` colunas da célula; o `bool` diz se algo foi cortado.
/// Evita quebrar em linhas um texto de milhões de caracteres que só mostraria `MAX_CELL_LINES`.
fn clip_pieces(cell: &[Piece], budget: usize) -> (Vec<Piece>, bool) {
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
fn wrap_cell(cell: &[Piece], w: usize) -> Vec<(Vec<Span<'static>>, usize)> {
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

/// Janela de `avail` colunas que começa na coluna `skip` de `pieces`. Marca com `‹`/`›` as
/// bordas onde há conteúdo cortado. Caractere largo que cruza a borda sai da janela (a linha
/// fica uma coluna mais curta; quem desenha completa com espaços).
fn window_by_width(pieces: &[Styled], skip: usize, avail: usize) -> Vec<Styled> {
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
mod testutil;
#[cfg(test)]
mod tests;
