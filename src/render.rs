//! Converte Markdown em linhas estilizadas (ratatui) já quebradas na largura pedida.
//!
//! O mesmo resultado serve para os dois modos do `mdv`: a TUI desenha as linhas
//! direto no `ratatui` e o modo `--plain` converte para ANSI e imprime.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

struct ThinkLine {
    opens: bool,
    closes: bool,
    rest: String,
}

fn think_tags(line: &str) -> Option<ThinkLine> {
    const OPEN: [&str; 2] = ["<think>", "<thinking>"];
    const CLOSE: [&str; 2] = ["</think>", "</thinking>"];
    let opens = OPEN.iter().any(|t| line.contains(t));
    let closes = CLOSE.iter().any(|t| line.contains(t));
    if !opens && !closes {
        return None;
    }
    let mut rest = line.to_string();
    for t in OPEN.iter().chain(CLOSE.iter()) {
        rest = rest.replace(t, "");
    }
    Some(ThinkLine { opens, closes, rest: rest.trim().to_string() })
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

/// Opções do `pulldown-cmark` usadas em todo o programa (renderização e extração veem a mesma estrutura).
pub(crate) fn parse_options() -> Options {
    // `ENABLE_MATH` fica de fora de propósito: textos de LLM com `$5 e $10` seriam lidos como fórmula.
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
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

/// Ponto único de entrada do texto do documento: troca caracteres de controle
/// (ESC, BEL, C1, `\r` solto...) por U+FFFD, para que nada do Markdown vire
/// comando de terminal. Preserva `\n` e `\t`; `\r\n` vira `\n`.
pub(crate) fn sanitize(s: &str) -> Cow<'_, str> {
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
    think: usize,
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
            think: 0,
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
            Event::InlineHtml(h) => match think_tags(&h) {
                Some(tl) => self.think_tag(tl.opens, tl.closes),
                None => self.inline.push_dim(h.to_string()),
            },
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
        let think_style = Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC);
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
            } else if let Some(tl) = think_tags(t) {
                if tl.opens {
                    text("💭 raciocínio".to_string(), dim.add_modifier(Modifier::ITALIC), &mut bodies);
                }
                if !tl.rest.is_empty() {
                    text(tl.rest, think_style, &mut bodies);
                }
                self.think_tag(tl.opens, tl.closes);
            } else {
                text(l.to_string(), dim, &mut bodies);
            }
        }
        self.push_prefixed(bodies);
    }

    /// Abre/fecha o estilo de "raciocínio" (itálico apagado) conforme as tags `<think>`.
    fn think_tag(&mut self, opens: bool, closes: bool) {
        if opens && !closes {
            self.think += 1;
            self.inline
                .push_style(Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC));
        } else if closes && !opens && self.think > 0 {
            self.think -= 1;
            self.inline.pop_style();
        }
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

        let start = self.out.lines.len();
        let header = if label.is_empty() { "╭─".to_string() } else { format!("╭─ {label}") };
        let mut v = first;
        v.push(Span::styled(header, dim));
        self.out.lines.push(Line::from(v));

        for line in LinesWithEndings::from(&block.buf) {
            if mem::exceeded() {
                break;
            }
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

        fn line_to_string(l: &Line<'_>) -> String {
            l.spans.iter().map(|s| s.content.as_ref()).collect()
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

        const FRASE_LONGA: &str = "uma descricao bem longa que precisa quebrar em varias linhas dentro da celula";

        fn tabela_de_uma_celula(texto: &str, alinhamento: &str) -> String {
            format!("| titulo |\n|{alinhamento}|\n| {texto} |\n")
        }

        #[test]
        fn celula_longa_quebra_em_varias_linhas() {
            let md = format!("| nome | descricao |\n|---|---|\n| a | {FRASE_LONGA} |\n");
            let lines = rendered_text_at(&md, 40);
            let junto = lines.join("\n");
            assert!(!junto.contains('…'), "não deve truncar: {junto}");
            for palavra in FRASE_LONGA.split(' ') {
                assert!(junto.contains(palavra), "faltou {palavra:?}: {junto}");
            }
            // borda, cabeçalho, separador, 3+ linhas de dado, borda
            assert!(lines.len() >= 7, "{lines:?}");
            assert_fits(&md, 40);
        }

        #[test]
        fn palavra_maior_que_a_coluna_quebra_por_caractere() {
            let md = tabela_de_uma_celula(&"x".repeat(100), "---");
            let junto = rendered_text_at(&md, 30).join("");
            assert_eq!(junto.matches('x').count(), 100, "{junto}");
            assert_fits(&md, 30);
        }

        #[test]
        fn teto_de_linhas_termina_com_reticencias() {
            let md = tabela_de_uma_celula(&"palavra ".repeat(200), "---");
            let lines = rendered_text_at(&md, 30);
            let dados: Vec<&String> = lines.iter().filter(|l| l.contains("palavra") || l.contains('…')).collect();
            assert_eq!(dados.len(), MAX_CELL_LINES, "{lines:?}");
            assert!(dados.last().unwrap().contains('…'), "{dados:?}");
            assert!(dados[..MAX_CELL_LINES - 1].iter().all(|l| !l.contains('…')), "{dados:?}");
        }

        #[test]
        fn celula_enorme_respeita_o_teto() {
            let md = tabela_de_uma_celula(&"x".repeat(500), "---");
            let lines = rendered_text_at(&md, 30);
            let dados: Vec<&String> = lines.iter().filter(|l| l.contains('x')).collect();
            assert_eq!(dados.len(), MAX_CELL_LINES, "{lines:?}");
            assert!(dados.last().unwrap().contains('…'), "{dados:?}");
        }

        #[test]
        fn alinhamento_a_direita_vale_em_cada_linha_da_celula() {
            let md = tabela_de_uma_celula("1234567890 1234567890", "--:");
            let lines = rendered_text_at(&md, 20);
            let dados: Vec<&String> = lines.iter().filter(|l| l.contains("1234567890")).collect();
            assert_eq!(dados.len(), 2, "{lines:?}");
            assert!(dados.iter().all(|l| l.ends_with("1234567890 │")), "{dados:?}");
        }

        #[test]
        fn todas_as_linhas_da_tabela_tem_a_mesma_largura() {
            let md = format!(
                "| a | b | c |\n|:--|:-:|--:|\n| {FRASE_LONGA} | 世界世界世界世界 | |\n| x | y | z |\n"
            );
            let larguras: std::collections::HashSet<usize> =
                render(&md, 40).lines.iter().filter(|l| !l.spans.is_empty()).map(Line::width).collect();
            assert_eq!(larguras.len(), 1, "{larguras:?}");
        }

        #[test]
        fn celula_com_estilo_mantem_o_estilo_apos_quebrar() {
            let md = tabela_de_uma_celula("**negrito comprido que quebra** fim", "---");
            let out = render(&md, 20);
            let negrito = out
                .lines
                .iter()
                .flat_map(|l| l.spans.iter())
                .filter(|s| s.style.add_modifier.contains(Modifier::BOLD) && s.content.contains("negrito"))
                .count();
            assert!(negrito >= 1);
        }

        #[test]
        fn tabela_so_de_dados_quebra_celula_longa() {
            let rows = vec![vec!["id".to_string(), FRASE_LONGA.to_string()]];
            let out = render_table_only(rows, false, vec![Alignment::Left; 2], 30);
            let junto: String = out.lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
            assert!(junto.contains("celula") && !junto.contains('…'), "{junto}");
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
    fn imagem_data_uri_vira_meio_bloco() {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let uri = format!("data:image/png;base64,{}", crate::clip::base64(&png.into_inner()));
        let store = ImageStore::new(None);
        let r = render_with(&format!("![x]({uri})"), &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        assert!(joined_text(&r).contains('▀'));
    }

    #[test]
    fn imagem_local_vira_meio_bloco() {
        let dir = tempfile::tempdir().unwrap();
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        std::fs::write(dir.path().join("a.png"), png.into_inner()).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("antes\n\n![logo](a.png)\n\ndepois", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        let t = joined_text(&r);
        assert!(t.contains('▀'), "{t}");
        assert!(!t.contains("[imagem"), "{t}");
        assert!(t.contains("antes") && t.contains("depois"));
    }

    #[test]
    fn imagem_quebrada_mostra_alt_e_motivo() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("![logo](nao.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        let t = joined_text(&r);
        assert!(t.contains("[imagem: logo"), "{t}");
        assert!(t.contains('—'), "{t}");
    }


    #[test]
    fn imagem_com_graficos_vira_placeholders_e_registra_o_png() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png"); // 4×4 px
        let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let r = render_with("![x](a.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        let t = joined_text(&r);
        assert!(t.contains('\u{10EEEE}') && !t.contains('▀'), "{t:?}");
        assert_eq!(r.graphics.len(), 1);
        let g = &r.graphics[0];
        assert_eq!((g.cols, g.rows), (4, 2));
        assert_eq!(r.lines.len(), g.rows);
        assert_eq!(&g.png[..4], b"\x89PNG");
    }

    #[test]
    fn mesma_imagem_duas_vezes_registra_um_png_so() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png");
        let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let r = render_with("![a](a.png)\n\n![b](a.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        assert_eq!(r.graphics.len(), 1);
        assert!(r.lines.iter().filter(|l| l.spans.iter().any(|s| s.content.contains('\u{10EEEE}'))).count() >= 4);
    }

    #[test]
    fn sem_graficos_continua_meio_bloco_e_sem_registro() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png");
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("![x](a.png)", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        assert!(joined_text(&r).contains('▀'));
        assert!(r.graphics.is_empty());
    }

    #[test]
    fn imagem_em_lista_com_graficos_mantem_o_prefixo() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png");
        let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let r = render_with("- item\n  ![x](a.png)\n", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        let img_line = r.lines.iter().find(|l| l.spans.iter().any(|s| s.content.contains('\u{10EEEE}'))).unwrap();
        assert!(img_line.spans.len() >= 2, "o recuo da lista vem antes dos placeholders");
    }

    fn png_no_dir(dir: &std::path::Path, nome: &str) {
        let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        std::fs::write(dir.join(nome), png.into_inner()).unwrap();
    }

    #[test]
    fn imagem_em_celula_vira_referencia_e_aparece_depois_da_tabela() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png");
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("| a |\n|---|\n| ![x](a.png) |\n\ndepois", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
        let t = joined_text(&r);
        assert!(t.contains("[imagem: x ↓1]"), "{t}");
        assert!(t.contains("↓1 x"), "{t}");
        assert!(t.contains('▀'), "{t}");
        assert!(t.find("[imagem: x ↓1]") < t.find('▀'), "a figura vem depois da tabela\n{t}");
        assert!(t.find('▀') < t.find("depois"), "{t}");
    }

    #[test]
    fn varias_imagens_na_tabela_sao_numeradas_na_ordem() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png");
        png_no_dir(dir.path(), "b.png");
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let md = "| c1 | c2 |\n|---|---|\n| ![um](a.png) | ![dois](b.png) |\n";
        let t = joined_text(&render_with(md, &RenderOpts { width: 60, images: Some(&store), ..Default::default() }));
        assert!(t.contains("↓1 um") && t.contains("↓2 dois"), "{t}");
        assert!(t.find("↓1 um") < t.find("↓2 dois"), "{t}");
    }

    #[test]
    fn imagem_quebrada_na_celula_mostra_o_motivo() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let t = joined_text(&render_with("| a |\n|---|\n| ![x](nao.png) |\n", &RenderOpts { width: 60, images: Some(&store), ..Default::default() }));
        assert!(t.contains("[imagem: x") && t.contains('—'), "{t}");
        assert!(!t.contains("↓1"), "{t}");
    }

    #[test]
    fn legenda_longa_da_figura_nao_passa_da_largura() {
        let dir = tempfile::tempdir().unwrap();
        png_no_dir(dir.path(), "a.png");
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let alt = "palavra ".repeat(30);
        let md = format!("| a |\n|---|\n| ![{alt}](a.png) |\n");
        let r = render_with(&md, &RenderOpts { width: 30, images: Some(&store), ..Default::default() });
        for l in &r.lines {
            let w: usize = l.spans.iter().map(|s| s.content.width()).sum();
            assert!(w <= 30, "linha com {w} colunas");
        }
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
        let r = render_with("```mermaid\ngraph TD; A-->B\n```", &RenderOpts { width: 40, images: Some(&store), ..Default::default() });
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
        let r = render_with("```mermaid\ngraph TD; A-->B\n```", &RenderOpts { width: 60, images: Some(&store), ..Default::default() });
        let t = joined_text(&r);
        assert!(t.contains("A-->B") && t.contains("erro"), "{t}");
    }

    fn t60(md: &str) -> String {
        joined_text(&render(md, 60))
    }

    #[test]
    fn codigo_sem_quebra_mantem_uma_linha_por_linha_do_fonte() {
        let md = format!("```\n{}\n```", "x".repeat(100));
        let r = render_with(&md, &RenderOpts { width: 40, code_scroll: Some(0), ..Default::default() });
        let t = joined_text(&r);
        assert_eq!(r.lines.len(), 3, "{t}"); // cabeçalho, 1 linha, rodapé
        assert!(t.contains('›') && !t.contains('‹'), "{t}");
    }

    #[test]
    fn codigo_com_deslocamento_mostra_o_trecho_e_as_duas_bordas() {
        // largura 20 => 18 colunas de código
        let md = "```\n0123456789abcdefghijklmnopqrstuvwxyz0123456789\n```";
        let r = render_with(md, &RenderOpts { width: 20, code_scroll: Some(10), ..Default::default() });
        let t = joined_text(&r);
        assert!(t.contains("‹bcdefghijklmnopq›"), "{t}");
        assert_eq!(r.code_overflow, 46 - 18);
    }

    #[test]
    fn codigo_curto_nao_ganha_marcas_nem_overflow() {
        let r = render_with("```\nab\n```", &RenderOpts { width: 20, code_scroll: Some(0), ..Default::default() });
        let t = joined_text(&r);
        assert!(!t.contains('›') && !t.contains('‹'), "{t}");
        assert_eq!(r.code_overflow, 0);
    }

    #[test]
    fn codigo_sem_opcao_continua_quebrando() {
        let md = format!("```\n{}\n```", "x".repeat(100));
        let r = render_with(&md, &RenderOpts { width: 40, ..Default::default() });
        assert!(r.lines.len() > 3);
    }

    #[test]
    fn janela_de_codigo_com_caractere_largo_na_borda_nao_estoura() {
        // "日" ocupa 2 colunas; a janela de 18 colunas nunca pode passar disso
        let md = format!("```\n{}\n```", "日".repeat(30));
        for skip in [0, 1, 5] {
            let r = render_with(&md, &RenderOpts { width: 20, code_scroll: Some(skip), ..Default::default() });
            for l in &r.lines {
                let w: usize = l.spans.iter().map(|s| s.content.width()).sum();
                assert!(w <= 20, "skip {skip}: linha com {w} colunas");
            }
        }
    }

    #[test]
    fn fonte_crua_tambem_aceita_deslocamento() {
        let r = render_code_only_at(&"y".repeat(100), "", 30, Some(0));
        assert_eq!(r.lines.len(), 3);
    }

    #[test]
    fn front_matter_vira_bloco_yaml() {
        let t = t60("---\ntitle: Plano\ntags: [a]\n---\n\n# Título");
        assert!(t.contains("yaml") && t.contains("title: Plano"), "{t}");
        assert!(!t.contains("---"), "{t}");
    }

    #[test]
    fn alerta_gfm_mostra_rotulo_em_portugues() {
        let t = t60("> [!WARNING]\n> cuidado aqui");
        assert!(t.contains("Aviso") && t.contains("cuidado aqui"), "{t}");
        assert!(!t.contains("[!WARNING]"), "{t}");
    }

    #[test]
    fn nota_de_rodape_aparece_no_texto_e_na_definicao() {
        let t = t60("Fato[^1].\n\n[^1]: Fonte aqui.");
        assert!(t.contains("[^1]") && t.contains("Fonte aqui"), "{t}");
    }

    #[test]
    fn details_e_summary_viram_titulo() {
        let t = t60("<details>\n<summary>Ver mais</summary>\n\nconteúdo\n\n</details>");
        assert!(t.contains("▸ Ver mais") && t.contains("conteúdo"), "{t}");
        assert!(!t.contains("<details>") && !t.contains("<summary>"), "{t}");
    }

    #[test]
    fn think_vira_bloco_rotulado() {
        let t = t60("<think>\n\nplano interno\n\n</think>\n\nresposta final");
        assert!(t.contains("raciocínio") && t.contains("plano interno") && t.contains("resposta final"), "{t}");
        assert!(!t.contains("<think>"), "{t}");
    }

    #[test]
    fn think_na_mesma_linha() {
        let t = t60("<think>só isto</think>\n\nok");
        assert!(t.contains("só isto") && !t.contains("</think>"), "{t}");
    }

    #[test]
    fn think_tags_parse() {
        let l = think_tags("<thinking>abc").unwrap();
        assert!(l.opens && !l.closes && l.rest == "abc");
        assert!(think_tags("<div>x</div>").is_none());
    }

    #[test]
    fn cerca_sem_fechar_nao_quebra() {
        let t = joined_text(&render("```rust\nfn main() {", 40));
        assert!(t.contains("fn main"), "{t}");
    }

    #[test]
    fn fonte_crua_mostra_markdown_literal() {
        let t = joined_text(&render_code_only("# a\n**b**\n", "md", 40));
        assert!(t.contains("# a") && t.contains("**b**") && t.contains("md"), "{t}");
    }

    fn todas(r: &Rendered) -> TaskCount {
        count_tasks(&r.tasks, 0, usize::MAX)
    }

    #[test]
    fn conta_tarefas_em_lista_aninhada() {
        let r = render("- [x] a\n- [ ] b\n  - [x] c\n", 40);
        assert_eq!(todas(&r), TaskCount { done: 2, total: 3 });
    }

    #[test]
    fn conta_tarefa_marcada_dentro_de_citacao() {
        let r = render("> - [x] q\n> - [ ] w\n", 40);
        assert_eq!(todas(&r), TaskCount { done: 1, total: 2 });
    }

    #[test]
    fn nao_conta_colchetes_em_codigo_nem_em_texto_corrido() {
        let r = render("```\n- [x] falso\n```\n\ntexto [ ] solto\n\n[x] sem lista\n", 40);
        assert_eq!(todas(&r), TaskCount::default());
    }

    #[test]
    fn count_tasks_usa_intervalo_fechado_aberto() {
        let tasks = [TaskItem { line: 2, done: true }, TaskItem { line: 5, done: false }];
        assert_eq!(count_tasks(&tasks, 2, 5), TaskCount { done: 1, total: 1 });
        assert_eq!(count_tasks(&tasks, 2, 6), TaskCount { done: 1, total: 2 });
        assert_eq!(count_tasks(&tasks, 3, 5), TaskCount::default());
    }

    #[test]
    fn secao_sem_tarefas_conta_zero() {
        let r = render("# A\n\n- [x] a\n\n# B\n\ntexto\n", 40);
        assert_eq!(section_tasks(&r, 0), TaskCount { done: 1, total: 1 });
        assert_eq!(section_tasks(&r, 1), TaskCount::default());
    }

    #[test]
    fn contagem_do_titulo_inclui_as_subsecoes() {
        let r = render("# A\n\n- [x] a\n\n## B\n\n- [ ] b\n\n## C\n\n- [x] c\n", 40);
        assert_eq!(section_tasks(&r, 0), TaskCount { done: 2, total: 3 });
        assert_eq!(section_tasks(&r, 1), TaskCount { done: 0, total: 1 });
        assert_eq!(section_tasks(&r, 2), TaskCount { done: 1, total: 1 });
    }

    #[test]
    fn tarefa_antes_do_primeiro_titulo_conta_so_no_documento() {
        let r = render("- [x] solta\n\n# A\n\ntexto\n", 40);
        assert_eq!(todas(&r), TaskCount { done: 1, total: 1 });
        assert_eq!(section_tasks(&r, 0), TaskCount::default());
    }

    #[test]
    fn toc_acrescenta_a_contagem_aos_titulos_com_tarefas() {
        let r = render("# A\n\n- [x] a\n\n## B\n\n- [ ] b\n\n# C\n\ntexto\n", 40);
        assert_eq!(toc(&r), "A [1/2]\n  B [0/1]\nC\n");
    }

    #[test]
    fn toc_indenta_por_nivel() {
        let r = render("# A\n\n## B\n\n### C\n\n## D", 40);
        assert_eq!(toc(&r), "A\n  B\n    C\n  D\n");
    }

    #[test]
    fn toc_sem_titulos_e_vazio() {
        assert_eq!(toc(&render("só texto", 40)), "");
    }

    #[test]
    fn toc_usa_texto_sem_formatacao_e_posicao_da_linha() {
        let r = render("texto\n\n## Um **forte** título", 40);
        assert_eq!(toc(&r), "  Um forte título\n");
        assert_eq!(r.outline[0].line, r.headings[0]);
    }

    #[test]
    fn erro_renderizado_tem_uma_linha_e_sanitiza() {
        assert_eq!(error_rendered("falhou").lines.len(), 1);
        assert!(!joined_text(&error_rendered("a\x1b[31mb")).contains('\x1b'));
    }

    #[test]
    fn escape_em_rotulo_de_nota_de_rodape_nao_vaza() {
        let r = render("Texto[^a\x1bcb].\n\n[^a\x1bcb]: nota\n", 60);
        assert!(!to_ansi(&r.lines, false).contains('\x1b'));
    }

    #[test]
    fn escape_em_motivo_de_falha_de_imagem_nao_vaza() {
        let url = "https://exemplo.invalido/k.png";
        let store = ImageStore::new(None).with_remote(crate::net::FetchPolicy::default(), true);
        store.get_job(url, false, || Err("falha\x1bc\x07".to_string()));
        let r = render_with(&format!("![x]({url})"), &RenderOpts { width: 60, images: Some(&store), ..Default::default() });
        let ansi = to_ansi(&r.lines, false);
        assert!(ansi.contains("falha") && !ansi.contains('\x1b') && !ansi.contains('\x07'), "{ansi:?}");
    }

    #[test]
    fn sem_store_mantem_comportamento_antigo() {
        let t = joined_text(&render("![logo](a.png)", 40));
        assert_eq!(t.trim(), "[imagem: logo]");
    }

        fn tabela_de_n_colunas(n: usize) -> String {
            let header: String = (1..=n).map(|i| format!(" c{i} |")).collect();
            let sep = "---|".repeat(n);
            let row: String = (1..=n).map(|i| format!(" v{i} |")).collect();
            format!("|{header}\n|{sep}\n|{row}\n")
        }

        #[test]
        fn render_registra_o_intervalo_de_cada_tabela() {
            let md = "antes\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\ndepois\n";
            let out = render(md, 40);
            assert_eq!(out.tables.len(), 1);
            let t = &out.tables[0];
            assert!(line_to_string(&out.lines[t.start]).starts_with('┌'));
            assert!(line_to_string(&out.lines[t.end - 1]).starts_with('└'));
            assert_eq!(t.data.total_cols(), 2);
        }

        #[test]
        fn tabela_em_citacao_e_lista_registra_o_intervalo_com_recuo() {
            let out = render("> | x |\n> |---|\n> | 1 |\n", 40);
            assert_eq!(out.tables.len(), 1);
            let t = &out.tables[0];
            assert!(line_to_string(&out.lines[t.start]).contains('┌'));
            let out = render("- item\n\n  | x |\n  |---|\n  | 1 |\n", 40);
            assert_eq!(out.tables.len(), 1);
            assert!(out.tables[0].end <= out.lines.len());
        }

        #[test]
        fn duas_tabelas_ficam_em_ordem_e_sem_sobreposicao() {
            let md = "| a |\n|---|\n| 1 |\n\ntexto\n\n| b |\n|---|\n| 2 |\n";
            let out = render(md, 40);
            assert_eq!(out.tables.len(), 2);
            assert!(out.tables[0].end <= out.tables[1].start);
        }

        #[test]
        fn render_table_only_registra_a_tabela() {
            let out = render_table_only(vec![vec!["a".into(), "b".into()]], false, vec![Alignment::Left; 2], 40);
            assert_eq!(out.tables.len(), 1);
            assert_eq!((out.tables[0].start, out.tables[0].end), (0, out.lines.len()));
        }

        #[test]
        fn draw_nao_tem_recuo_e_fixa_cabecalho() {
            let out = render("> | x | y |\n> |---|---|\n> | 1 | 2 |\n", 40);
            let v = out.tables[0].data.draw(40, 0);
            assert!(line_to_string(&v.lines[0]).starts_with('┌'), "sem o '▎ ' da citação");
            assert_eq!(v.pinned, 3, "borda, cabeçalho e separador");
            assert_eq!((v.first_col, v.shown_cols, v.total_cols), (1, 2, 2));
        }

        #[test]
        fn draw_sem_cabecalho_fixa_so_a_borda() {
            let out = render_table_only(vec![vec!["a".into()]], false, vec![Alignment::Left], 40);
            assert_eq!(out.tables[0].data.draw(40, 0).pinned, 1);
        }

        #[test]
        fn draw_com_uma_coluna_nao_estoura_o_indice() {
            let out = render("| só |\n|---|\n| 1 |\n", 40);
            let v = out.tables[0].data.draw(20, usize::MAX);
            assert_eq!((v.first_col, v.shown_cols, v.total_cols), (0, 1, 1));
        }

        #[test]
        fn draw_rola_por_coluna_e_mantem_a_primeira() {
            let out = render(&tabela_de_n_colunas(12), 20);
            let data = &out.tables[0].data;

            let v = data.draw(20, 0);
            assert_eq!((v.first_col, v.shown_cols), (1, 3));
            let cab = line_to_string(&v.lines[1]);
            assert!(cab.contains("│ c1 ") && cab.contains("c2"), "{cab}");
            assert!(line_to_string(v.lines.last().unwrap()).contains("+9 colunas"));

            let v = data.draw(20, 5);
            assert_eq!(v.first_col, 5);
            let cab = line_to_string(&v.lines[1]);
            assert!(cab.contains("│ c1 ") && cab.contains("c6") && !cab.contains("c2 "), "{cab}");

            let v = data.draw(20, usize::MAX);
            assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols, "última coluna à vista");
            assert!(line_to_string(&v.lines[1]).contains("c12"));
        }

        #[test]
        fn draw_cabe_em_qualquer_largura_pedida() {
            let out = render(&tabela_de_n_colunas(30), 80);
            for w in [20, 33, 80, 200] {
                for first in [0, 3, usize::MAX] {
                    let v = out.tables[0].data.draw(w, first);
                    assert!(v.lines.iter().all(|l| l.width() <= w), "largura {w}, coluna {first}");
                }
            }
        }

    fn linha_de(r: &Rendered, i: usize) -> String {
        r.lines[i].spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn indice_da_linha_com(r: &Rendered, trecho: &str) -> usize {
        (0..r.lines.len())
            .find(|&i| linha_de(r, i).contains(trecho))
            .unwrap_or_else(|| panic!("nenhuma linha contém {trecho:?}"))
    }

    #[test]
    fn line_src_tem_uma_entrada_por_linha_renderizada() {
        let md = "# T\n\npar um\n\n- a\n- b\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> citação\n";
        let r = render(md, 40);
        assert_eq!(r.line_src.len(), r.lines.len());
    }

    #[test]
    fn line_src_aponta_para_a_linha_do_fonte() {
        let r = render("# Título\n\nprimeiro\n\nsegundo parágrafo\n", 40);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "Título")], 0);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "primeiro")], 2);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "segundo")], 4);
    }

    #[test]
    fn line_src_de_lista_aninhada_e_citacao_usa_a_linha_de_cada_item() {
        let r = render("- a\n  - b\n- c\n\n> q1\n> q2\n", 40);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "a")], 0);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "b")], 1);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "c")], 2);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "q1")], 4);
    }

    #[test]
    fn linha_quebrada_pela_largura_herda_a_linha_do_bloco() {
        let md = format!("# T\n\n{}\n\nfim\n", "palavra ".repeat(30));
        let r = render(&md, 30);
        let do_bloco: Vec<usize> = (0..r.lines.len())
            .filter(|&i| linha_de(&r, i).contains("palavra"))
            .map(|i| r.line_src[i])
            .collect();
        assert!(do_bloco.len() > 1, "o parágrafo deveria quebrar em várias linhas");
        assert!(do_bloco.iter().all(|&s| s == 2), "{do_bloco:?}");
    }

    #[test]
    fn bloco_de_codigo_registra_intervalo_linguagem_e_texto_cru() {
        let r = render("x\n\n```rust\nfn a() {}\n```\n\n~~~\nsem lang\n~~~\n", 40);
        assert_eq!(r.code_blocks.len(), 2);
        let (a, b) = (&r.code_blocks[0], &r.code_blocks[1]);
        assert_eq!(a.lang, "rust");
        assert_eq!(a.text, "fn a() {}\n");
        assert!(linha_de(&r, a.start).starts_with("╭─"));
        assert!(linha_de(&r, a.end - 1).starts_with("╰─"));
        assert_eq!(b.lang, "");
        assert_eq!(b.text, "sem lang\n");
    }

    #[test]
    fn tarefas_registram_feita_ou_aberta_na_linha_do_item() {
        let r = render("# T\n\n- [x] a\n- [ ] b\n  - [x] c\n\n> - [ ] d\n", 40);
        let feitas: Vec<bool> = r.tasks.iter().map(|t| t.done).collect();
        assert_eq!(feitas, vec![true, false, true, false]);
        for t in &r.tasks {
            let marca = if t.done { '☑' } else { '☐' };
            assert!(linha_de(&r, t.line).contains(marca), "linha {}: {:?}", t.line, linha_de(&r, t.line));
        }
    }

    #[test]
    fn section_range_vai_ate_o_proximo_titulo_de_nivel_igual_ou_menor() {
        let r = render("# A\n\ntexto\n\n## B\n\nb\n\n## C\n\nc\n\n# D\n\nd\n", 40);
        let o = &r.outline;
        assert_eq!(section_range(o, 0, r.lines.len()), (o[0].line, o[3].line));
        assert_eq!(section_range(o, 1, r.lines.len()), (o[1].line, o[2].line));
        assert_eq!(section_range(o, 3, r.lines.len()), (o[3].line, r.lines.len()));
    }

    #[test]
    fn section_at_acha_o_titulo_mais_profundo_antes_da_linha() {
        let r = render("antes\n\n# A\n\ntexto\n\n## B\n\nb\n", 40);
        let o = &r.outline;
        assert_eq!(section_at(o, 0), None, "antes do primeiro título");
        assert_eq!(section_at(o, o[0].line), Some(0));
        assert_eq!(section_at(o, indice_da_linha_com(&r, "texto")), Some(0));
        assert_eq!(section_at(o, indice_da_linha_com(&r, "b")), Some(1));
    }

    #[test]
    fn section_source_devolve_o_markdown_cru_com_o_titulo() {
        let md = "intro\n\n# A\n\ntexto\n\n## B\n\nb\n\n# C\n\nc\n";
        let r = render(md, 40);
        assert_eq!(section_source(md, &r, 0).as_deref(), Some("# A\n\ntexto\n\n## B\n\nb\n"));
        assert_eq!(section_source(md, &r, 1).as_deref(), Some("## B\n\nb\n"));
        assert_eq!(section_source(md, &r, 2).as_deref(), Some("# C\n\nc\n"), "última seção vai ao fim");
        assert_eq!(section_source(md, &r, 9), None);
    }

    #[test]
    fn section_source_sem_mapa_de_linhas_devolve_none() {
        let r = error_rendered("x");
        assert_eq!(section_source("# A\n", &r, 0), None);
    }
}
