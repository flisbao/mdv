mod browse;
mod cli;
mod clip;
mod data;
mod doc;
mod graphics;
#[cfg(test)]
mod golden_ansi;
#[cfg(test)]
mod app_keys_tests;
mod imgview;
mod imgzoom;
mod load;
mod image;
mod mem;
mod mermaid_native;
mod nav;
mod net;
mod picker;
mod plain;
mod stats;
mod stream;
mod render;

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use doc::{DocOpts, Document};
use load::{image_store, load_source, read_markdown_file, reads_stdin, Source, MAX_INPUT_BYTES};
use plain::{run_plain, write_stdout, HELP};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

#[global_allocator]
static ALLOC: mem::CountingAlloc = mem::CountingAlloc;
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

/// Colunas deixadas livres nas laterais da TUI.
const TUI_MARGIN: usize = 4;
/// Menor largura de texto na TUI, mesmo em terminais minúsculos.
const MIN_TUI_WIDTH: usize = 10;
/// Intervalo entre verificações em segundo plano (download de imagem terminado).
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Intervalo entre verificações com texto chegando por stdin (`--follow`).
const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(80);

/// Falha que encerra o programa; o código de saída depende do tipo.
enum Failure {
    /// Uso incorreto da linha de comando (código 2).
    Usage(String),
    /// Erro ao ler a entrada ou usar o terminal (código 1).
    Runtime(String),
}

impl Failure {
    fn exit_code(&self) -> ExitCode {
        match self {
            Failure::Usage(_) => ExitCode::from(2),
            Failure::Runtime(_) => ExitCode::from(1),
        }
    }
}

fn main() -> ExitCode {
    match run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            match &failure {
                Failure::Usage(msg) => eprintln!("mdv: {msg}\nUse `mdv --help` para ver as opções."),
                Failure::Runtime(msg) => eprintln!("mdv: {msg}"),
            }
            failure.exit_code()
        }
    }
}

fn run_cli() -> Result<(), Failure> {
    let cli = match cli::parse(std::env::args().skip(1)).map_err(Failure::Usage)? {
        cli::Parsed::Help => return write_stdout(HELP),
        cli::Parsed::Version => {
            return write_stdout(&format!("mdv {}\n", env!("CARGO_PKG_VERSION")));
        }
        cli::Parsed::Run(cli) => cli,
    };
    let limit = mem::resolve(cli.memory_buffer, std::env::var("MDV_MEMORY_BUFFER").ok().as_deref())
        .map_err(Failure::Usage)?;
    mem::set_limit(limit);
    mem::init();
    let mem_warning = mem::warn_if_above(limit, mem::available());
    let stdout_tty = io::stdout().is_terminal();
    let interactive = !cli.plain && stdout_tty;
    let width = cli.width.unwrap_or(render::MAX_WIDTH);
    if !interactive {
        if let Some(w) = &mem_warning {
            eprintln!("mdv: {w}");
        }
    }

    // `--follow` com stdin: a TUI abre na hora e o texto vai chegando aos poucos.
    if cli.follow && interactive && !cli.toc && reads_stdin(cli.path.as_deref()) {
        let rx = stream::spawn_stdin_reader(usize::try_from(MAX_INPUT_BYTES).unwrap_or(usize::MAX));
        let mut app = App::new(Source::Stdin, String::new(), width, &cli).with_stream(rx);
        app.notice = app.notice.or(mem_warning);
        return run_tui(app);
    }

    if interactive && !cli.toc && cli.mode == cli::Mode::Markdown {
        if let Some(dir) = cli.path.as_deref().map(Path::new).filter(|p| p.is_dir()) {
            let picker = picker::Picker::scan(dir)
                .map_err(|e| Failure::Runtime(format!("não consegui ler {}: {e}", dir.display())))?;
            let mut app = App::new(Source::Stdin, String::new(), width, &cli);
            app.name = render::sanitize(&dir.display().to_string()).into_owned();
            app.picker = Some(picker);
            app.notice = app.notice.or(mem_warning);
            return run_tui(app);
        }
    }
    let (source, md) = load_source(cli.path.as_deref())?;
    if cli.stats {
        return write_stdout(&stats::format(&stats::stats(&md)));
    }
    if cli.toc && cli.mode == cli::Mode::Markdown {
        return write_stdout(&render::toc(&render::render(&md, width)));
    }
    if interactive {
        let mut app = App::new(source, md, width, &cli);
        app.notice = app.notice.or(mem_warning);
        run_tui(app)
    } else {
        run_plain(&md, &source, cli.width, stdout_tty, &cli)
    }
}

fn run_tui(mut app: App) -> Result<(), Failure> {
    let mut term = ratatui::init();
    let res = event_loop(&mut term, &mut app);
    if !app.sent_graphics.is_empty() || app.zoom_sent {
        emit_terminal_control(graphics::DELETE_ALL);
    }
    ratatui::restore();
    res.map_err(|e| Failure::Runtime(format!("erro no terminal: {e}")))
}

/// Escreve uma sequência de controle direto no terminal (OSC 52 para cópia, APC para Kitty).
fn emit_terminal_control(seq: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Largura de uma célula em pixels (8 se o terminal não informa o tamanho da janela).
fn cell_px_width() -> f64 {
    match terminal::window_size() {
        Ok(ws) if ws.width > 0 && ws.columns > 0 => f64::from(ws.width) / f64::from(ws.columns),
        _ => 8.0,
    }
}

fn event_loop(term: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        let size = term.size()?;
        app.prepare(size.width);
        app.prepare_img_zoom(size.width, size.height, cell_px_width());
        if !app.pending_gfx.is_empty() {
            emit_terminal_control(&std::mem::take(&mut app.pending_gfx));
        }
        term.draw(|f| app.draw(f))?;
        let interval = if app.stream.is_some() { STREAM_POLL_INTERVAL } else { POLL_INTERVAL };
        if event::poll(interval)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && app.on_key(k) {
                    return Ok(());
                }
                if let Some(seq) = app.pending_osc.take() {
                    emit_terminal_control(&seq);
                }
            }
        } else {
            app.drain_stream();
            app.poll_background();
            if let Some(seq) = app.pending_osc.take() {
                emit_terminal_control(&seq);
            }
        }
    }
}

/// Janela de rolagem sobre as linhas renderizadas.
#[derive(Default)]
struct Viewport {
    scroll: usize,
    height: usize,
}

impl Viewport {
    fn max_scroll(&self, total: usize) -> usize {
        total.saturating_sub(self.height)
    }

    fn scroll_by(&mut self, delta: isize, total: usize) {
        let target = if delta.is_negative() {
            self.scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.scroll.saturating_add(delta.unsigned_abs())
        };
        self.scroll_to(target, total);
    }

    fn scroll_to(&mut self, line: usize, total: usize) {
        self.scroll = line.min(self.max_scroll(total));
    }
}

/// Busca incremental: texto em edição, termo confirmado e linhas encontradas.
/// Tela de zoom: uma tabela desenhada na largura inteira do terminal.
struct Zoom {
    /// Índice em `Rendered.tables`.
    table: usize,
    /// Primeira coluna rolável pedida (a coluna 0 fica sempre; `draw` normaliza).
    first_col: usize,
    /// Rolagem do corpo da tabela, sem as linhas fixas do topo.
    view: Viewport,
    /// Último desenho: (largura, `first_col` usado, resultado).
    drawn: Option<(usize, usize, render::TableView)>,
}

impl Zoom {
    fn new(table: usize) -> Self {
        Zoom { table, first_col: 0, view: Viewport::default(), drawn: None }
    }

    /// Linhas do corpo (tudo que não é fixo no topo).
    fn body_total(&self) -> usize {
        self.drawn.as_ref().map_or(0, |(_, _, v)| v.lines.len().saturating_sub(v.pinned))
    }
}

/// Índice do intervalo `[start, end)` com mais linhas dentro da janela (empate: o primeiro).
/// Escolher o primeiro que toca a janela esconde os blocos seguintes quando um é mais alto que a tela.
fn most_visible(spans: impl Iterator<Item = (usize, usize)>, window: std::ops::Range<usize>) -> Option<usize> {
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
fn section_title(r: &render::Rendered, line: usize) -> Option<String> {
    render::section_at(&r.outline, line).map(|i| render::sanitize(&r.outline[i].title).into_owned())
}

/// Próximo/anterior índice entre `n` itens, dando a volta nos extremos; `None` com menos de 2 itens.
fn step_index(cur: usize, n: usize, forward: bool) -> Option<usize> {
    if n < 2 {
        return None;
    }
    Some(if forward { (cur + 1) % n } else { (cur + n - 1) % n })
}

/// "todas as N colunas" ou "colunas 1 e A–B de N" (1 é a coluna fixa).
fn columns_label(v: &render::TableView) -> String {
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

#[derive(Default)]
struct SearchState {
    /// Texto sendo digitado (`Some` enquanto o prompt `/` está aberto).
    input: Option<String>,
    query: String,
    matches: Vec<usize>,
    current: usize,
}

impl SearchState {
    fn refresh(&mut self, lines: &[Line<'_>]) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        let q = fold(&self.query);
        for (i, l) in lines.iter().enumerate() {
            if find_from(&fold(&line_text(l)), &q, 0).is_some() {
                self.matches.push(i);
            }
        }
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    /// Confirma `query` e escolhe a primeira ocorrência a partir de `scroll`.
    fn commit(&mut self, query: String, lines: &[Line<'_>], scroll: usize) {
        self.query = query;
        self.refresh(lines);
        self.current = self.matches.iter().position(|&m| m >= scroll).unwrap_or(0);
    }

    fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
    }

    fn step(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = if forward { (self.current + 1) % n } else { (self.current + n - 1) % n };
    }

    fn current_line(&self) -> Option<usize> {
        self.matches.get(self.current).copied()
    }
}

struct App {
    source: Source,
    images: image::ImageStore,
    seen_images_version: u64,
    raw: bool,
    doc: Document,
    stream: Option<Receiver<String>>,
    stick_bottom: bool,
    name: String,
    max_width: usize,
    view: Viewport,
    search: SearchState,
    zoom: Option<Zoom>,
    /// Zoom de imagem (`i`): um diagrama Mermaid na tela inteira.
    img_zoom: Option<imgview::ImgZoom>,
    /// O zoom de imagem já enviou algo ao terminal (limpar ao sair).
    zoom_sent: bool,
    notice: Option<String>,
    open_table: Option<usize>,
    pending_osc: Option<String>,
    cli: cli::Cli,
    /// Link selecionado: (linha renderizada, índice em `Rendered.links`).
    link_at: Option<(usize, usize)>,
    /// URL externa esperando confirmação (`y`).
    pending_open: Option<String>,
    back: Vec<browse::Back>,
    /// Seletor de arquivos aberto (`mdv <pasta>`); enquanto existe, ocupa a tela.
    picker: Option<picker::Picker>,
    pending_jump: Option<browse::Jump>,
    /// Ids gráficos (Kitty) já enviados ao terminal.
    sent_graphics: HashSet<u32>,
    /// Sequências Kitty ainda não escritas no terminal.
    pending_gfx: String,
}

impl App {
    fn new(source: Source, md: String, max_width: usize, cli: &cli::Cli) -> Self {
        let name = match &source {
            Source::File(p) => render::sanitize(
                &p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
            )
            .into_owned(),
            Source::Stdin => "stdin".to_string(),
        };
        App {
            images: image_store(&source, cli, false),
            seen_images_version: 0,
            raw: false,
            doc: Document::new(md, DocOpts::from_cli(cli)),
            stream: None,
            stick_bottom: false,
            source,
            name,
            max_width: render::clamp_width(max_width),
            view: Viewport::default(),
            search: SearchState::default(),
            zoom: None,
            img_zoom: None,
            zoom_sent: false,
            notice: None,
            open_table: cli.table.map(|n| n - 1),
            pending_osc: None,
            cli: cli.clone(),
            link_at: None,
            pending_open: None,
            back: Vec::new(),
            picker: None,
            pending_jump: None,
            sent_graphics: HashSet::new(),
            pending_gfx: String::new(),
        }
    }

    /// Passa a receber o texto aos poucos (`--follow`).
    fn with_stream(mut self, rx: Receiver<String>) -> Self {
        self.stream = Some(rx);
        // Com o texto ainda chegando, um bloco mermaid pode estar incompleto: só gera no fim.
        self.images.set_mermaid_hold(true);
        self
    }

    /// Acrescenta o que chegou do stdin; só passa a acompanhar o fim se o leitor já estava nele.
    fn drain_stream(&mut self) {
        let Some(rx) = &self.stream else { return };
        let was_at_bottom = self.view.scroll >= self.view.max_scroll(self.total_lines());
        let mut got = false;
        let mut finished = false;
        let mut chunks = String::new();
        loop {
            match rx.try_recv() {
                Ok(chunk) => {
                    chunks.push_str(&chunk);
                    got = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if !chunks.is_empty() {
            self.doc.append_md(&chunks);
        }
        self.stop_stream_for_memory(mem::check().err());
        if finished {
            self.stream = None;
            self.images.set_mermaid_hold(false);
            got = true;
        }
        if got {
            self.stick_bottom |= was_at_bottom;
            self.doc.dirty = true;
        }
    }

    /// Para de acumular o stdin quando a memória passou do limite (`--follow`).
    fn stop_stream_for_memory(&mut self, over: Option<mem::MemoryLimit>) {
        if let Some(e) = over {
            if self.stream.take().is_some() {
                self.images.set_mermaid_hold(false);
                self.notice = Some(format!("leitura interrompida: {e}"));
                self.doc.dirty = true;
            }
        }
    }

    /// Depois de redesenhar, leva a tela ao fim se o leitor estava acompanhando o fim.
    fn apply_stick_bottom(&mut self) {
        if self.stick_bottom {
            self.view.scroll = self.view.max_scroll(self.total_lines());
            self.stick_bottom = false;
        }
    }

    fn total_lines(&self) -> usize {
        self.doc.rendered.lines.len()
    }

    fn scroll_by(&mut self, delta: isize) {
        self.view.scroll_by(delta, self.total_lines());
    }

    fn scroll_to(&mut self, line: usize) {
        self.view.scroll_to(line, self.total_lines());
    }

    /// Linhas renderizadas que cabem na janela.
    fn window(&self) -> std::ops::Range<usize> {
        let total = self.total_lines();
        self.view.scroll.min(total)..(self.view.scroll + self.view.height).min(total)
    }

    /// As linhas da janela como serão desenhadas: com o destaque da busca.
    fn screen_lines(&self) -> Vec<Line<'static>> {
        let q = self.search.query.as_str();
        self.doc.rendered.lines[self.window()]
            .iter()
            .map(|l| {
                let mut l = if q.is_empty() { l.clone() } else { highlight_line(l, q) };
                self.decorate_links(&mut l);
                l
            })
            .collect()
    }

    /// Copia o bloco de código sob a linha de referência (`y`).
    fn copy_code(&mut self) {
        let top = self.view.scroll;
        let janela = self.window();
        let achado = self
            .doc
            .rendered
            .code_blocks
            .iter()
            .find(|c| c.start <= top && top < c.end)
            .or_else(|| self.doc.rendered.code_blocks.iter().find(|c| c.start < janela.end && c.end > janela.start))
            .map(|c| c.text.clone());
        let Some(text) = achado else {
            self.notice = Some("nenhum bloco de código".to_string());
            return;
        };
        self.copy_text(&text);
    }

    /// Copia o Markdown cru da seção da linha de referência (`Y`).
    fn copy_section(&mut self) {
        if self.raw || self.doc.opts.mode != cli::Mode::Markdown {
            self.notice = Some("seção só existe no Markdown renderizado".to_string());
            return;
        }
        let Some(idx) = render::section_at(&self.doc.rendered.outline, self.view.scroll) else {
            self.notice = Some("nenhuma seção aqui".to_string());
            return;
        };
        match render::section_source(&self.doc.md, &self.doc.rendered, idx) {
            Some(text) => self.copy_text(&text),
            None => self.notice = Some("não consegui recortar a seção".to_string()),
        }
    }

    /// Prepara o OSC 52 (escrito pelo `event_loop`) ou avisa que o texto é grande demais.
    fn copy_text(&mut self, text: &str) {
        match clip::osc52(text) {
            Ok(seq) => {
                self.pending_osc = Some(seq);
                let n = text.lines().count();
                self.notice = Some(format!("copiado: {n} linha{}", if n == 1 { "" } else { "s" }));
            }
            Err(bytes) => {
                self.notice = Some(format!("grande demais ({} KiB), nada copiado", bytes / 1024));
            }
        }
    }

    fn rerender(&mut self, w: usize) {
        self.doc.render(w, self.raw, &self.images);
        if self.link_at.is_some_and(|(l, id)| l >= self.doc.rendered.lines.len() || id >= self.doc.rendered.links.len()) {
            self.link_at = None;
        }
        self.search.refresh(&self.doc.rendered.lines);
        self.view.scroll = self.view.scroll.min(self.view.max_scroll(self.total_lines()));
        let n = self.doc.rendered.tables.len();
        if self.zoom.as_ref().is_some_and(|z| z.table >= n) {
            self.zoom = None;
        } else if let Some(z) = &mut self.zoom {
            z.drawn = None;
        }
        self.sync_img_zoom();
        self.queue_graphics();
    }

    /// Trabalho de fundo a cada tique: um download de imagem terminou, então redesenha com ela.
    fn poll_background(&mut self) {
        let version = self.images.version();
        if version != self.seen_images_version {
            self.seen_images_version = version;
            self.doc.dirty = true;
        }
    }

    /// Relê o arquivo; se a leitura falhar, mantém o documento atual.
    fn reload(&mut self) {
        let Source::File(p) = &self.source else { return };
        if let Ok(md) = read_markdown_file(p) {
            self.doc.set_md(md);
        }
    }

    fn set_search(&mut self, query: String) {
        self.search.commit(query, &self.doc.rendered.lines, self.view.scroll);
        self.goto_match();
    }

            /// Abre o zoom pedido por `--table`, assim que há um documento renderizado.
        fn open_pending_table(&mut self) {
            if let Some(i) = self.open_table.take() {
                self.open_zoom(i);
            }
        }

    fn open_zoom(&mut self, table: usize) {
            if table >= self.doc.rendered.tables.len() {
                self.notice = Some(format!(
                    "tabela {} não existe (há {})",
                    table + 1,
                    self.doc.rendered.tables.len()
                ));
                return;
            }
            self.notice = None;
            self.zoom = Some(Zoom::new(table));
        }

        /// Abre o zoom da primeira tabela que aparece na tela.
        fn open_visible_table(&mut self) {
            let achada = most_visible(self.doc.rendered.tables.iter().map(|t| (t.start, t.end)), self.window());
            match achada {
                Some(i) => self.open_zoom(i),
                None => self.notice = Some("nenhuma tabela na tela".to_string()),
            }
        }

        /// Redesenha a tabela do zoom se a largura ou a coluna mudaram.
        fn refresh_zoom(&mut self, width: usize) {
            let Some(z) = self.zoom.as_mut() else { return };
            let Some(span) = self.doc.rendered.tables.get(z.table) else { return };
            let fresh = matches!(&z.drawn, Some((w, c, _)) if *w == width && *c == z.first_col);
            if !fresh {
                let v = span.data.draw(width, z.first_col);
                z.first_col = v.first_col;
                z.drawn = Some((width, v.first_col, v));
            }
        }

        fn on_zoom_key(&mut self, code: KeyCode) {
            let Some(z) = self.zoom.as_mut() else { return };
            let body = z.body_total();
            let h = z.view.height;
            let page = isize::try_from(h.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
            let half = isize::try_from((h / 2).max(1)).unwrap_or(isize::MAX);
            match code {
                KeyCode::Esc | KeyCode::Char('q' | 't') => self.zoom = None,
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

    /// `w`: alterna entre código quebrado na largura e código sem quebra, rolável com `h`/`l`.
    fn toggle_code_wrap(&mut self) {
        self.doc.opts.code_scroll = match self.doc.opts.code_scroll {
            None => Some(0),
            Some(_) => None,
        };
        self.doc.dirty = true;
    }

    /// Vai para a coluna `to` do código (limitada ao fim do código mais largo).
    fn set_code_scroll(&mut self, to: usize) {
        let Some(cur) = self.doc.opts.code_scroll else {
            self.notice = Some("w: código sem quebra (h/l rolam)".into());
            return;
        };
        let max = self.doc.rendered.code_overflow;
        let next = to.min(max);
        if next != cur {
            self.doc.opts.code_scroll = Some(next);
            self.doc.dirty = true;
        } else if max == 0 {
            self.notice = Some("nenhum código largo aqui".into());
        }
    }

    fn scroll_code(&mut self, step: usize, forward: bool) {
        let Some(cur) = self.doc.opts.code_scroll else {
            self.set_code_scroll(0);
            return;
        };
        let to = if forward { cur.saturating_add(step) } else { cur.saturating_sub(step) };
        self.set_code_scroll(to);
    }

    fn goto_match(&mut self) {
        if let Some(line) = self.search.current_line() {
            self.scroll_to(line.saturating_sub(self.view.height / 3));
        }
    }

    fn step_match(&mut self, forward: bool) {
        self.search.step(forward);
        self.goto_match();
    }

    /// Retorna `true` quando o usuário pediu para sair.
    fn on_key(&mut self, k: KeyEvent) -> bool {
        if self.picker.is_some() {
            if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                return true;
            }
            return self.on_picker_key(k);
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return true;
        }
                if self.img_zoom.is_some() {
                    self.on_img_zoom_key(k.code);
                    return false;
                }
                if self.zoom.is_some() {
                    self.on_zoom_key(k.code);
                    return false;
                }
                if self.pending_open.is_some() {
                    self.confirm_open(k.code == KeyCode::Char('y'));
                    return false;
                }
                self.notice = None;
        if let Some(buf) = self.search.input.take() {
            self.on_search_key(k.code, buf);
            return false;
        }

        let height = self.view.height;
        let page = isize::try_from(height.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
        let half = isize::try_from((height / 2).max(1)).unwrap_or(isize::MAX);
        match k.code {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                if self.link_at.is_some() {
                    self.link_at = None;
                } else if self.search.query.is_empty() {
                    return true;
                } else {
                    self.search.clear();
                }
            }
            KeyCode::Tab => self.step_link(true),
            KeyCode::BackTab => self.step_link(false),
            KeyCode::Enter if self.link_at.is_some() => self.follow_link(),
            KeyCode::Backspace | KeyCode::Char('H') => self.go_back(),
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('d') => self.scroll_by(half),
            KeyCode::Char('u') => self.scroll_by(-half),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(0),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(usize::MAX),
            KeyCode::Char(']') => {
                let top = self.view.scroll;
                let alvo = self.doc.rendered.headings.iter().copied().find(|&h| h > top);
                if let Some(h) = alvo {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('[') => {
                let top = self.view.scroll;
                let alvo = self.doc.rendered.headings.iter().rev().copied().find(|&h| h < top);
                if let Some(h) = alvo {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('/') => self.search.input = Some(String::new()),
            KeyCode::Char('n') => self.step_match(true),
            KeyCode::Char('N') => self.step_match(false),
            KeyCode::Char('r') => self.reload(),
                        KeyCode::Char('v') if self.doc.opts.mode == cli::Mode::Json => {
                            let current = if self.doc.opts.json_view == cli::JsonView::Auto {
                                data::auto_view_of_text(&self.doc.md, self.doc.opts.json_path.as_deref())
                            } else {
                                self.doc.opts.json_view
                            };
                            self.doc.opts.json_view = data::next_view(current);
                            self.doc.dirty = true;
                        }
                        KeyCode::Char('t') => self.open_visible_table(),
                        KeyCode::Char('i') => self.open_visible_mermaid(),
                        KeyCode::Char('y') => self.copy_code(),
                        KeyCode::Char('Y') => self.copy_section(),
            KeyCode::Char('w') => self.toggle_code_wrap(),
            KeyCode::Char('l') | KeyCode::Right => self.scroll_code(8, true),
            KeyCode::Char('h') | KeyCode::Left => self.scroll_code(8, false),
            KeyCode::Char('0') => self.set_code_scroll(0),
            KeyCode::Char('$') => self.set_code_scroll(usize::MAX),
            KeyCode::Char('m') => {
                self.raw = !self.raw;
                self.doc.dirty = true;
            }
            _ => {}
        }
        false
    }

    /// Trata uma tecla com o prompt de busca aberto (`buf` é o texto digitado até agora).
    fn on_search_key(&mut self, code: KeyCode, mut buf: String) {
        match code {
            KeyCode::Esc => {}
            KeyCode::Enter => self.set_search(buf),
            KeyCode::Backspace => {
                buf.pop();
                self.search.input = Some(buf);
            }
            KeyCode::Char(c) => {
                buf.push(c);
                self.search.input = Some(buf);
            }
            _ => self.search.input = Some(buf),
        }
    }

    /// Largura do texto para uma tela de `screen` colunas.
    fn text_width(&self, screen: u16) -> usize {
        usize::from(screen)
            .saturating_sub(TUI_MARGIN)
            .min(self.max_width)
            .max(MIN_TUI_WIDTH)
    }

    /// Renderiza antes de desenhar, para que as imagens gráficas cheguem ao terminal
    /// **antes** dos placeholders que as mostram.
    fn prepare(&mut self, screen_width: u16) {
        let w = self.text_width(screen_width);
        if self.doc.needs_render(w) {
            self.rerender(w);
        }
    }

    /// Enfileira as imagens do documento atual que o terminal ainda não recebeu.
    fn queue_graphics(&mut self) {
        for g in &self.doc.rendered.graphics {
            if self.sent_graphics.insert(g.id) {
                self.pending_gfx.push_str(&graphics::transmit(g.id, &g.png, g.cols, g.rows));
            }
        }
    }

    fn draw(&mut self, f: &mut Frame) {
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
        self.view.scroll = self.view.scroll.min(self.view.max_scroll(self.total_lines()));
        self.apply_stick_bottom();

        let pad = usize::from(content.width).saturating_sub(w) / 2;
        let inner = Rect {
            x: content.x.saturating_add(u16::try_from(pad).unwrap_or(u16::MAX)),
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

    fn status_line(&self, width: usize) -> Line<'static> {
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

    /// Nome do arquivo, posição no documento e estado da busca.
        fn draw_zoom(&mut self, f: &mut Frame, content: Rect, status: Rect) {
            self.refresh_zoom(render::clamp_width(usize::from(content.width)));
            let Some(z) = self.zoom.as_mut() else { return };
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

            let bar = Style::default().bg(Color::Rgb(59, 66, 82)).fg(Color::Rgb(229, 233, 240));
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
            let text = if lw + rw < width { format!("{left}{}{right}", " ".repeat(width - lw - rw)) } else { left };
            f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
        }

    /// `☑ feitas/total` do documento e, se a seção atual tem tarefas e difere do todo, `(seção f/t)`.
    fn task_summary(&self) -> Option<String> {
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

    fn status_left(&self) -> String {
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

fn line_text(l: &Line<'_>) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Minúscula caractere a caractere (um `char` vira um `char`): índices de `Vec<char>` continuam
/// alinhados com a linha original mesmo onde `to_lowercase` mudaria o tamanho (ex.: `İ`).
fn fold_char(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

fn fold(s: &str) -> Vec<char> {
    s.chars().map(fold_char).collect()
}

/// Primeira posição de `needle` em `hay` a partir de `from`. `needle` vazio nunca casa.
fn find_from(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// Destaca as ocorrências da busca `q` (sem diferenciar caixa) na linha, inclusive as que
/// atravessam trechos de estilos diferentes: cada caractere guarda o estilo do seu trecho.
fn highlight_line(line: &Line<'static>, q: &str) -> Line<'static> {
    let needle = fold(q);
    if needle.is_empty() {
        return line.clone();
    }
    let cells: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();
    let hay: Vec<char> = cells.iter().map(|&(c, _)| fold_char(c)).collect();
    let mut marked = vec![false; cells.len()];
    let mut start = 0;
    let mut any = false;
    while let Some(i) = find_from(&hay, &needle, start) {
        marked[i..i + needle.len()].fill(true);
        start = i + needle.len();
        any = true;
    }
    if !any {
        return line.clone();
    }
    let hl = Style::default().bg(Color::Rgb(255, 213, 79)).fg(Color::Black);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut cur: Option<Style> = None;
    for (&(c, st), &m) in cells.iter().zip(&marked) {
        let style = if m { st.patch(hl) } else { st };
        if cur != Some(style) {
            if let Some(prev) = cur {
                out.push(Span::styled(std::mem::take(&mut buf), prev));
            }
            cur = Some(style);
        }
        buf.push(c);
    }
    if let Some(prev) = cur {
        out.push(Span::styled(buf, prev));
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;


    #[test]
    fn stdin_para_de_acumular_quando_a_memoria_acaba() {
        let (_tx, rx) = std::sync::mpsc::channel::<String>();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        app.stop_stream_for_memory(None);
        assert!(app.stream.is_some(), "sem estouro, segue lendo");
        app.stop_stream_for_memory(Some(mem::MemoryLimit { used: 2, limit: 1 }));
        assert!(app.stream.is_none());
        assert!(app.notice.as_deref().is_some_and(|n| n.contains("leitura interrompida")));
        assert!(!app.images.mermaid_held(), "libera os diagramas");
    }

    #[test]
    fn destaque_com_busca_vazia_nao_altera_a_linha() {
        let line = Line::from(vec![Span::raw("abc"), Span::raw("def")]);
        assert_eq!(highlight_line(&line, ""), line);
    }

    #[test]
    fn destaque_marca_cada_ocorrencia() {
        let line = Line::from("a-b-a");
        let out = highlight_line(&line, "a");
        let marked: Vec<&str> = out
            .spans
            .iter()
            .filter(|s| s.style.bg.is_some())
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(marked, ["a", "a"]);
        assert_eq!(line_text(&out), "a-b-a");
    }

    #[test]
    fn destaque_atravessa_trechos_estilizados() {
        let bold = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
        let line = Line::from(vec![Span::raw("foo"), Span::styled("bar", bold), Span::raw(" baz")]);
        let out = highlight_line(&line, "foobar");
        assert_eq!(line_text(&out), "foobar baz");
        let marked: String = out
            .spans
            .iter()
            .filter(|s| s.style.bg.is_some())
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(marked, "foobar");
        // o trecho em negrito continua em negrito, agora também destacado
        assert!(out.spans.iter().any(|s| s.content == "bar"
            && s.style.add_modifier.contains(ratatui::style::Modifier::BOLD)
            && s.style.bg.is_some()));
    }

    #[test]
    fn destaque_ignora_caixa_e_nao_desalinha_com_i_turco() {
        let line = Line::from("İx e X");
        let out = highlight_line(&line, "x");
        assert_eq!(line_text(&out), "İx e X");
        let marked: Vec<&str> = out
            .spans
            .iter()
            .filter(|s| s.style.bg.is_some())
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(marked, ["x", "X"]);
    }

    #[test]
    fn busca_acha_linha_com_texto_dividido_entre_trechos() {
        let lines = vec![
            Line::from(vec![Span::raw("foo"), Span::raw("bar")]),
            Line::from("outra"),
        ];
        let mut s = SearchState::default();
        s.commit("FooBar".to_string(), &lines, 0);
        assert_eq!(s.matches, [0]);
    }

    fn test_app(source: Source, md: &str) -> App {
        let cli = match cli::parse(["x.md".to_string()]) {
            Ok(cli::Parsed::Run(c)) => c,
            _ => panic!("cli de teste"),
        };
        App::new(source, md.to_string(), 80, &cli)
    }

    /// Texto de todas as células do buffer, uma linha por string.
    fn screen_text(buf: &ratatui::buffer::Buffer) -> Vec<String> {
        let w = buf.area.width;
        (0..buf.area.height)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    /// Ponta a ponta: diagrama Mermaid nativo → tecla `i` → zoom desenhado → `Esc` volta.
    #[test]
    fn tecla_i_abre_o_zoom_do_diagrama_mermaid() {
        use ratatui::{backend::TestBackend, Terminal};
        if !crate::mermaid_native::fonts_available() {
            return;
        }
        let md = "# D\n\n```mermaid\nflowchart LR\n  A[Início] --> B[Fim]\n```\n";
        let mut app = test_app(Source::Stdin, md);
        app.images = image::ImageStore::new(None).with_native_mermaid(true).with_graphics(true);
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        assert_eq!(app.doc.rendered.mermaids.len(), 1, "o diagrama virou imagem");
        app.on_key(ev(KeyCode::Char('i')));
        assert!(app.img_zoom.is_some(), "{:?}", app.notice);
        app.prepare_img_zoom(100, 30, 8.0);
        term.draw(|f| app.draw(f)).unwrap();
        let text = screen_text(term.backend().buffer()).join("\n");
        assert!(text.contains("diagrama 1/1"), "{text}");
        app.on_key(ev(KeyCode::Char('+')));
        app.on_key(ev(KeyCode::Esc));
        assert!(app.img_zoom.is_none());
    }

    #[test]
    fn tecla_i_sem_diagrama_avisa() {
        let mut app = test_app(Source::Stdin, "# só texto\n");
        app.prepare(80);
        app.on_key(ev(KeyCode::Char('i')));
        assert!(app.img_zoom.is_none());
        assert_eq!(app.notice.as_deref(), Some("nenhum diagrama na tela"));
    }

    /// Regressão da troca do ratatui: desenha a TUI num `TestBackend` e confere conteúdo, estilo e rolagem.
    #[test]
    fn tui_desenha_documento_no_test_backend() {
        use ratatui::{backend::TestBackend, Terminal};
        let md = "# Título\n\nTexto **forte** e `código`.\n\n- um\n- dois\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let mut app = test_app(Source::Stdin, md);
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        let linhas = screen_text(&buf);
        let tela = linhas.join("\n");
        for esperado in ["Título", "forte", "código", "um", "dois", "fn main", "│"] {
            assert!(tela.contains(esperado), "faltou {esperado:?} na tela:\n{tela}");
        }
        let fg_titulo = (0..buf.area.width).find(|&x| buf[(x, 0)].symbol() != " ").map(|x| buf[(x, 0)].fg);
        assert!(fg_titulo.is_some_and(|c| c != ratatui::style::Color::Reset), "título sem cor");
        // redimensionar e redesenhar não pode entrar em pânico nem perder o conteúdo
        let mut pequeno = Terminal::new(TestBackend::new(30, 8)).unwrap();
        pequeno.draw(|f| app.draw(f)).unwrap();
        assert!(screen_text(pequeno.backend().buffer()).join("\n").contains("Título"));
    }

    fn key(app: &mut App, c: KeyCode) {
        app.on_key(KeyEvent::new(c, KeyModifiers::NONE));
    }

    #[test]
    fn tab_enter_segue_link_md_e_backspace_volta() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.doc.md");
        fs::write(&a, "# A\n\n[ir](b.md#segundo)\n").unwrap();
        fs::write(dir.path().join("b.md"), "# B\n\ntexto\n\n## Segundo\n\nfim\n").unwrap();
        let mut app = test_app(Source::File(a.clone()), "# A\n\n[ir](b.md#segundo)\n");
        app.view.height = 3;
        app.rerender(60);
        key(&mut app, KeyCode::Tab);
        assert!(app.link_at.is_some());
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.name, "b.md");
        app.rerender(60);
        app.apply_pending_jump();
        let seg = app.doc.rendered.outline[1].line;
        assert!(app.view.scroll <= seg && seg < app.view.scroll + 3, "rolou até a âncora");
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.name, "a.doc.md");
        assert!(app.back.is_empty());
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.notice.as_deref(), Some("nada para voltar"));
    }

    #[test]
    fn backspace_restaura_o_modo_do_documento_anterior() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("d.json");
        fs::write(&json, "{\"a\":1}\n").unwrap();
        let mut app = test_app(Source::Stdin, "# A\n");
        app.back.push(browse::Back::File { path: json.clone(), line: 0, mode: cli::Mode::Json });
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.doc.opts.mode, cli::Mode::Json);
        assert!(matches!(&app.source, Source::File(p) if *p == json));
    }

    #[test]
    fn link_externo_so_abre_com_y_e_link_fora_da_pasta_e_recusado() {
        let md = "[x](https://exemplo.org) e [y](/etc/passwd.md)\n";
        let mut app = test_app(Source::Stdin, md);
        app.view.height = 5;
        app.rerender(60);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.pending_open.as_deref(), Some("https://exemplo.org"));
        key(&mut app, KeyCode::Char('n'));
        assert!(app.pending_open.is_none());
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert!(app.pending_open.is_none());
        assert!(app.notice.as_deref().unwrap_or("").starts_with("não abri o link"), "{:?}", app.notice);
    }

    #[test]
    fn marcas_de_link_nao_chegam_a_tela() {
        let mut app = test_app(Source::Stdin, "[x](#a)\n\n# a\n");
        app.view.height = 5;
        app.rerender(60);
        key(&mut app, KeyCode::Tab);
        let lines = app.screen_lines();
        assert!(lines.iter().flat_map(|l| &l.spans).all(|s| s.style.underline_color.is_none()));
        assert!(lines.iter().flat_map(|l| &l.spans).any(|s| s.style.add_modifier.contains(ratatui::style::Modifier::REVERSED)));
    }

    #[test]
    fn seletor_abre_arquivo_e_backspace_volta_a_ele() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("um.md"), "# Um\n").unwrap();
        let mut app = test_app(Source::Stdin, "");
        app.picker = Some(picker::Picker::scan(dir.path()).unwrap());
        for c in "um".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        key(&mut app, KeyCode::Enter);
        assert!(app.picker.is_none());
        assert_eq!(app.name, "um.md");
        key(&mut app, KeyCode::Backspace);
        let p = app.picker.as_ref().expect("voltou ao seletor");
        assert_eq!(p.query, "um", "a consulta é mantida");
        assert!(!app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)), "q digita no filtro");
        assert!(app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn stream_acumula_pedacos_e_marca_para_redesenhar() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        app.doc.dirty = false;
        tx.send("# um\n".to_string()).unwrap();
        tx.send("dois".to_string()).unwrap();
        app.drain_stream();
        assert_eq!(app.doc.md, "# um\ndois");
        assert!(app.doc.dirty);
    }

    #[test]
    fn diagramas_esperam_o_fim_do_stdin_em_streaming() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        assert!(app.images.mermaid_held(), "enquanto o texto chega, não dispara o diagrama");
        tx.send("# x\n".to_string()).unwrap();
        app.drain_stream();
        assert!(app.images.mermaid_held());
        drop(tx);
        app.doc.dirty = false;
        app.drain_stream();
        assert!(!app.images.mermaid_held(), "entrada terminou: libera os diagramas");
        assert!(app.doc.dirty, "redesenha para os diagramas aparecerem");
    }

    #[test]
    fn barra_de_status_explica_que_as_teclas_pulam_de_titulo() {
        let app = test_app(Source::Stdin, "# a");
        let barra: String = app.status_line(120).spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(barra.contains("[ ] título"), "{barra}");
    }

    #[test]
    fn stream_sem_novidade_nao_redesenha() {
        let (_tx, rx) = std::sync::mpsc::channel::<String>();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        app.doc.dirty = false;
        app.drain_stream();
        assert!(!app.doc.dirty);
    }

    #[test]
    fn stream_acompanha_o_fim_so_se_ja_estava_no_fim() {
        let (tx, rx) = std::sync::mpsc::channel();
        let long: String = (0..30).map(|i| format!("linha {i}\n\n")).collect();
        let mut app = test_app(Source::Stdin, &long).with_stream(rx);
        app.rerender(40);
        app.view.height = 5;

        app.view.scroll = 0;
        tx.send("mais\n".to_string()).unwrap();
        app.drain_stream();
        assert!(!app.stick_bottom, "leitor no meio do texto não deve ser arrastado");

        app.view.scroll = app.view.max_scroll(app.total_lines());
        tx.send("fim\n".to_string()).unwrap();
        app.drain_stream();
        assert!(app.stick_bottom);
        app.rerender(40);
        app.apply_stick_bottom();
        assert_eq!(app.view.scroll, app.view.max_scroll(app.total_lines()));
        assert!(!app.stick_bottom);
    }

        fn app_kitty(dir: &std::path::Path) -> App {
            let img = ::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([9, 9, 9, 255])));
            let mut png = std::io::Cursor::new(Vec::new());
            img.write_to(&mut png, ::image::ImageFormat::Png).unwrap();
            std::fs::write(dir.join("a.png"), png.into_inner()).unwrap();
            let md = dir.join("a.doc.md");
            std::fs::write(&md, "![x](a.png)\n").unwrap();
            let cli = match cli::parse(["--graphics", "kitty", "a.doc.md"].map(String::from)) {
                Ok(cli::Parsed::Run(c)) => c,
                _ => panic!("cli de teste"),
            };
            App::new(Source::File(md), "![x](a.png)\n".to_string(), 80, &cli)
        }

        #[test]
        fn prepare_enfileira_a_imagem_uma_vez_so() {
            let dir = tempfile::tempdir().unwrap();
            let mut app = app_kitty(dir.path());
            app.prepare(100);
            assert!(app.pending_gfx.starts_with("\x1b_Ga=T"), "{:?}", app.pending_gfx);
            assert_eq!(app.sent_graphics.len(), 1);
            app.pending_gfx.clear();
            app.doc.dirty = true;
            app.prepare(100);
            assert!(app.pending_gfx.is_empty(), "já foi enviada");
        }

        #[test]
        fn sem_graphics_nada_e_enfileirado() {
            let mut app = test_app(Source::Stdin, "![x](nao.png)");
            app.prepare(100);
            assert!(app.pending_gfx.is_empty() && app.sent_graphics.is_empty());
        }

        fn tecla(app: &mut App, c: char) {
            app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        #[test]
        fn v_alterna_as_vistas_do_json() {
            let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
            app.doc.opts.mode = cli::Mode::Json;
            tecla(&mut app, 'v'); // Auto já era tabela: o próximo é o esquema
            assert_eq!(app.doc.opts.json_view, cli::JsonView::Schema);
            assert!(app.doc.dirty);
            tecla(&mut app, 'v');
            assert_eq!(app.doc.opts.json_view, cli::JsonView::Text);
            tecla(&mut app, 'v');
            assert_eq!(app.doc.opts.json_view, cli::JsonView::Table);
        }

        #[test]
        fn v_em_json_que_nao_e_lista_comeca_pela_tabela() {
            let mut app = test_app(Source::Stdin, r#"{"a":1}"#);
            app.doc.opts.mode = cli::Mode::Json;
            tecla(&mut app, 'v');
            assert_eq!(app.doc.opts.json_view, cli::JsonView::Table);
        }

        #[test]
        fn v_fora_do_modo_json_nao_faz_nada() {
            let mut app = test_app(Source::Stdin, "# a");
            app.doc.dirty = false;
            tecla(&mut app, 'v');
            assert_eq!(app.doc.opts.json_view, cli::JsonView::Auto);
            assert!(!app.doc.dirty);
        }

        #[test]
        fn barra_mostra_a_vista_do_json_quando_escolhida() {
            let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
            app.doc.opts.mode = cli::Mode::Json;
            tecla(&mut app, 'v');
            assert!(app.status_left().contains("[esquema]"), "{}", app.status_left());
        }

        #[test]
        fn w_liga_codigo_sem_quebra_e_hl_rolam() {
            let longa = "x".repeat(200);
            let mut app = test_app(Source::Stdin, &format!("```\n{longa}\n```"));
            app.rerender(60); // 58 colunas de código => overflow 142
            assert_eq!(app.doc.rendered.code_overflow, 142);
            tecla(&mut app, 'w');
            assert_eq!(app.doc.opts.code_scroll, Some(0));
            assert!(app.doc.dirty);
            app.rerender(60);
            tecla(&mut app, 'l');
            assert_eq!(app.doc.opts.code_scroll, Some(8));
            tecla(&mut app, 'h');
            tecla(&mut app, 'h');
            assert_eq!(app.doc.opts.code_scroll, Some(0));
            tecla(&mut app, '$');
            assert_eq!(app.doc.opts.code_scroll, Some(142));
            tecla(&mut app, '0');
            assert_eq!(app.doc.opts.code_scroll, Some(0));
            tecla(&mut app, 'w');
            assert_eq!(app.doc.opts.code_scroll, None);
        }

        #[test]
        fn hl_sem_w_avisam_e_nao_mudam_nada() {
            let mut app = test_app(Source::Stdin, "```\nabc\n```");
            app.rerender(60);
            tecla(&mut app, 'l');
            assert_eq!(app.doc.opts.code_scroll, None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains('w')), "{:?}", app.notice);
        }

        #[test]
        fn rolagem_nao_passa_do_fim_do_codigo() {
            let mut app = test_app(Source::Stdin, &format!("```\n{}\n```", "x".repeat(70)));
            app.rerender(60); // overflow 12
            tecla(&mut app, 'w');
            for _ in 0..5 {
                tecla(&mut app, 'l');
            }
            assert_eq!(app.doc.opts.code_scroll, Some(12));
        }

        #[test]
        fn barra_de_status_mostra_o_modo_sem_quebra() {
            let mut app = test_app(Source::Stdin, "```\nabc\n```");
            app.rerender(60);
            tecla(&mut app, 'w');
            assert!(app.status_left().contains("sem quebra"), "{}", app.status_left());
        }

        fn app_com_tabela(md: &str) -> App {
            let mut app = test_app(Source::Stdin, md);
            app.rerender(60);
            app.view.height = 10;
            app
        }

        fn ev(c: KeyCode) -> KeyEvent {
            KeyEvent::new(c, KeyModifiers::NONE)
        }

        fn app_com_texto(md: &str) -> App {
            let mut app = test_app(Source::Stdin, md);
            app.rerender(60);
            app.view.height = 10;
            app
        }

        fn copiado(app: &App) -> Option<String> {
            app.pending_osc.clone()
        }

        #[test]
        fn rodape_mostra_o_andamento_das_tarefas_do_documento() {
            let app = app_com_texto("- [x] a\n- [ ] b\n- [ ] c\n");
            assert!(app.status_left().contains("☑ 1/3"), "{}", app.status_left());
        }

        #[test]
        fn rodape_sem_tarefas_nao_mostra_contador() {
            let app = app_com_texto("# A\n\ntexto\n");
            assert!(!app.status_left().contains('☑'), "{}", app.status_left());
        }

        #[test]
        fn rodape_acrescenta_a_secao_atual_quando_ela_tem_tarefas() {
            let mut app = app_com_texto("# A\n\n- [x] a\n\n# B\n\n- [ ] b\n- [ ] c\n");
            app.view.scroll = app.doc.rendered.outline[1].line;
            let s = app.status_left();
            assert!(s.contains("☑ 1/3") && s.contains("(seção 0/2)"), "{s}");
        }

        #[test]
        fn rodape_nao_repete_a_contagem_quando_a_secao_e_o_documento() {
            let app = app_com_texto("# A\n\n- [x] a\n- [ ] b\n");
            let s = app.status_left();
            assert!(s.contains("☑ 1/2") && !s.contains("seção"), "{s}");
        }

        #[test]
        fn rodape_com_tarefa_antes_do_primeiro_titulo_nao_quebra() {
            let app = app_com_texto("- [x] solta\n\n# A\n\ntexto\n");
            assert!(app.status_left().contains("☑ 1/1"));
        }

        #[test]
        fn rodape_no_modo_fonte_nao_mostra_contador() {
            let mut app = app_com_texto("- [x] a\n");
            app.on_key(ev(KeyCode::Char('m')));
            app.rerender(60);
            assert!(!app.status_left().contains('☑'), "{}", app.status_left());
        }

        #[test]
        fn y_copia_o_bloco_de_codigo_da_tela() {
            let mut app = app_com_texto("texto\n\n```rust\nfn a() {}\n```\n");
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), Some(clip::osc52("fn a() {}\n").unwrap()));
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("copiado: 1 linha")), "{:?}", app.notice);
        }

        #[test]
        fn y_sem_bloco_de_codigo_avisa_e_nao_copia() {
            let mut app = app_com_texto("só texto\n");
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhum bloco de código")));
        }

        #[test]
        fn y_em_json_nao_entra_em_panico_e_avisa() {
            let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
            app.doc.opts.mode = cli::Mode::Json;
            app.rerender(60);
            app.view.height = 10;
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.is_some());
        }

        #[test]
        fn y_no_modo_fonte_copia_o_documento() {
            let mut app = app_com_texto("# A\n\ntexto\n");
            app.on_key(ev(KeyCode::Char('m')));
            app.rerender(60);
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), Some(clip::osc52("# A\n\ntexto\n").unwrap()));
        }

        #[test]
        fn y_maiusculo_copia_o_markdown_cru_da_secao() {
            let mut app = app_com_texto("# A\n\ntexto\n\n## B\n\nb\n");
            app.on_key(ev(KeyCode::Char('Y')));
            assert_eq!(copiado(&app), Some(clip::osc52("# A\n\ntexto\n\n## B\n\nb\n").unwrap()));
        }

        #[test]
        fn y_maiusculo_usa_a_secao_da_primeira_linha_visivel() {
            let mut app = app_com_texto("# A\n\na\n\n# B\n\nb\n");
            let b = app.doc.rendered.outline[1].line;
            app.view.scroll = b;
            app.on_key(ev(KeyCode::Char('Y')));
            assert_eq!(copiado(&app), Some(clip::osc52("# B\n\nb\n").unwrap()));
        }

        #[test]
        fn y_maiusculo_sem_titulo_avisa_e_nao_copia() {
            let mut app = app_com_texto("só texto\n");
            app.on_key(ev(KeyCode::Char('Y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma seção")));
        }

        #[test]
        fn copia_maior_que_64_kib_nao_envia_nada() {
            let md = format!("```\n{}\n```\n", "x".repeat(70 * 1024));
            let mut app = app_com_texto(&md);
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("grande demais (70 KiB)")), "{:?}", app.notice);
        }

        const TABELA: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";

        fn tabela_larga(n: usize) -> String {
            let h: String = (1..=n).map(|i| format!(" c{i} |")).collect();
            format!("|{h}\n|{}\n|{h}\n", "---|".repeat(n))
        }

        #[test]
        fn mais_visivel_prefere_o_bloco_com_mais_linhas_na_tela() {
            // janela 10..30: bloco 0 mostra 5 linhas (5..15), bloco 1 mostra 20 (15..60)
            assert_eq!(most_visible([(5, 15), (15, 60)].into_iter(), 10..30), Some(1));
            assert_eq!(most_visible([(100, 120)].into_iter(), 10..30), None);
            assert_eq!(most_visible([(10, 20), (20, 30)].into_iter(), 10..30), Some(0), "empate: o primeiro");
        }

        #[test]
        fn n_e_p_trocam_de_tabela_no_zoom_dando_a_volta() {
            let md = format!("{TABELA}\n{}\n{TABELA}", "texto\n\n".repeat(30));
            let mut app = app_com_tabela(&md);
            app.on_key(ev(KeyCode::Char('t')));
            app.on_key(ev(KeyCode::Char('p')));
            assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(1), "do primeiro volta ao último");
            app.on_key(ev(KeyCode::Char('n')));
            assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(0), "do último vai ao primeiro");
            assert_eq!(step_index(0, 1, true), None);
        }

        #[test]
        fn t_abre_o_zoom_da_tabela_visivel() {
            let mut app = app_com_tabela(TABELA);
            assert!(!app.on_key(ev(KeyCode::Char('t'))));
            assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(0));
        }

        #[test]
        fn t_sem_tabela_na_tela_avisa_e_nao_abre() {
            let mut app = app_com_tabela("# só texto\n");
            app.on_key(ev(KeyCode::Char('t')));
            assert!(app.zoom.is_none());
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma tabela")));
            app.on_key(ev(KeyCode::Char('j')));
            assert!(app.notice.is_none(), "o aviso some na próxima tecla");
        }

        #[test]
        fn t_escolhe_a_tabela_que_aparece_na_tela() {
            let md = format!("{TABELA}\n{}\n{TABELA}", "texto\n\n".repeat(30));
            let mut app = app_com_tabela(&md);
            let segunda = app.doc.rendered.tables[1].start;
            app.view.scroll = segunda.saturating_sub(1);
            app.on_key(ev(KeyCode::Char('t')));
            assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(1));
        }

        #[test]
        fn esc_e_q_fecham_o_zoom_sem_sair() {
            for k in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('t')] {
                let mut app = app_com_tabela(TABELA);
                app.on_key(ev(KeyCode::Char('t')));
                assert!(!app.on_key(ev(k)), "{k:?} no zoom só fecha");
                assert!(app.zoom.is_none());
            }
        }

        #[test]
        fn ctrl_c_sai_mesmo_no_zoom() {
            let mut app = app_com_tabela(TABELA);
            app.on_key(ev(KeyCode::Char('t')));
            assert!(app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        }

        #[test]
        fn colunas_rolam_com_h_l_zero_e_cifrao() {
            let mut app = app_com_tabela(&tabela_larga(12));
            app.on_key(ev(KeyCode::Char('t')));
            app.refresh_zoom(20);
            let first = |app: &App| app.zoom.as_ref().unwrap().first_col;
            assert_eq!(first(&app), 1);

            app.on_key(ev(KeyCode::Char('l')));
            app.refresh_zoom(20);
            assert_eq!(first(&app), 2);

            app.on_key(ev(KeyCode::Char('$')));
            app.refresh_zoom(20);
            let v = app.zoom.as_ref().unwrap().drawn.as_ref().map(|d| &d.2).unwrap();
            assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols);

            app.on_key(ev(KeyCode::Right));
            app.refresh_zoom(20);
            let v = app.zoom.as_ref().unwrap().drawn.as_ref().map(|d| &d.2).unwrap();
            assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols, "não passa da última");

            app.on_key(ev(KeyCode::Char('0')));
            app.refresh_zoom(20);
            assert_eq!(first(&app), 1);

            app.on_key(ev(KeyCode::Left));
            app.refresh_zoom(20);
            assert_eq!(first(&app), 1, "não anda para antes da primeira");
        }

        #[test]
        fn rolagem_vertical_do_zoom_nao_passa_do_fim() {
            let linhas: String = (0..40).map(|i| format!("| {i} |\n")).collect();
            let mut app = app_com_tabela(&format!("| n |\n|---|\n{linhas}"));
            app.on_key(ev(KeyCode::Char('t')));
            app.refresh_zoom(30);
            app.on_key(ev(KeyCode::Char('G')));
            let z = app.zoom.as_ref().unwrap();
            assert!(z.view.scroll > 0);
            app.on_key(ev(KeyCode::Char('g')));
            assert_eq!(app.zoom.as_ref().unwrap().view.scroll, 0);
        }

        #[test]
        fn recarregar_com_a_tabela_sumida_fecha_o_zoom() {
            let mut app = app_com_tabela(TABELA);
            app.on_key(ev(KeyCode::Char('t')));
            app.doc.md = "sem tabela\n".to_string();
            app.rerender(60);
            assert!(app.zoom.is_none());
        }

        #[test]
        fn rerender_com_a_tabela_ainda_presente_mantem_o_zoom_e_refaz_o_desenho() {
            let mut app = app_com_tabela(TABELA);
            app.on_key(ev(KeyCode::Char('t')));
            app.refresh_zoom(30);
            app.rerender(60);
            let z = app.zoom.as_ref().expect("continua aberto");
            assert!(z.drawn.is_none(), "o cache do desenho é descartado");
        }

        #[test]
        fn rotulo_de_colunas() {
            let md = tabela_larga(12);
            let out = render::render(&md, 20);
            let data = &out.tables[0].data;
            assert_eq!(columns_label(&data.draw(200, 0)), "todas as 12 colunas");
            assert_eq!(columns_label(&data.draw(20, 0)), "colunas 1 e 2–3 de 12");
            let so_uma = render::render("| a |\n|---|\n| 1 |\n", 20);
            assert_eq!(columns_label(&so_uma.tables[0].data.draw(20, 0)), "1 coluna");
        }

        fn app_com_cli(args: &[&str], md: &str) -> App {
            let cli = match cli::parse(args.iter().map(|s| s.to_string())) {
                Ok(cli::Parsed::Run(c)) => c,
                _ => panic!("cli de teste"),
            };
            App::new(Source::Stdin, md.to_string(), 80, &cli)
        }

        #[test]
        fn table_n_abre_o_zoom_na_primeira_renderizacao() {
            let md = format!("{TABELA}\ntexto\n\n{TABELA}");
            let mut app = app_com_cli(&["--table", "2", "-"], &md);
            app.rerender(60);
            app.open_pending_table();
            assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(1));
            assert!(app.open_table.is_none());
        }

        #[test]
        fn table_alem_do_que_existe_avisa_e_nao_abre() {
            let mut app = app_com_cli(&["--table", "5", "-"], TABELA);
            app.rerender(60);
            app.open_pending_table();
            assert!(app.zoom.is_none());
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("tabela 5 não existe (há 1)")));
        }
}
