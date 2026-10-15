mod cli;
mod data;
mod image;
mod mermaid;
mod net;
mod stream;
mod render;

use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, SystemTime};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::terminal;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{DefaultTerminal, Frame};
use unicode_width::UnicodeWidthStr;

/// Teto de bytes lidos de arquivo ou stdin (protege a memória).
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Largura máxima do texto quando `-w` não é informado.
const DEFAULT_WIDTH: usize = 100;
/// Largura do `--plain` quando o tamanho do terminal é desconhecido.
const FALLBACK_TERMINAL_WIDTH: usize = 80;
/// Colunas deixadas livres nas laterais da TUI.
const TUI_MARGIN: usize = 4;
/// Menor largura de texto na TUI, mesmo em terminais minúsculos.
const MIN_TUI_WIDTH: usize = 10;
/// Intervalo entre verificações de alteração do arquivo (live reload).
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Intervalo entre verificações com texto chegando por stdin (`--follow`).
const STREAM_POLL_INTERVAL: Duration = Duration::from_millis(80);

const HELP: &str = "\
mdv - visualizador de Markdown no terminal

USO:
    mdv [opções] <arquivo.md>
    cat arquivo.md | mdv
    mdv json [opções] <arquivo|->
    mdv csv|tsv [opções] <arquivo|->
    (o formato também é deduzido da extensão: .json, .jsonl, .csv, .tsv)

OPÇÕES:
    -p, --plain        imprime no terminal (sem TUI), como `cat` com cores
    -w, --width <N>    largura máxima do texto (padrão: 100)
        --remote       baixa imagens de http(s):// (nunca de rede privada)
        --mermaid      renderiza blocos ```mermaid via `mmdc` (ou $MDV_MMDC)
        --toc          imprime só o índice de títulos
        --follow       com stdin: acompanha o texto conforme chega
        --latest       o argumento é uma pasta: abre e segue o .md mais novo
    -h, --help         mostra esta ajuda
    -V, --version      mostra a versão

OPÇÕES DE JSON:
        --path <p>     mostra só um trecho, ex.: .itens[0].nome
        --as <vista>   auto (padrão), text, table ou schema

OPÇÕES DE CSV/TSV:
        --head <N>     mostra só as N primeiras linhas (padrão: 1000)
        --cols <a,b>   mostra só estas colunas, pelo nome do cabeçalho
        --no-header    a primeira linha é dado, não cabeçalho
        --delimiter <c>  separador de um caractere (ou `tab`)

TECLAS (modo interativo):
    j / k, ↓ / ↑       rola uma linha
    espaço / b         página abaixo / acima
    d / u              meia página
    g / G              início / fim
    ] / [              próximo / título anterior
    /                  buscar (Enter confirma, Esc cancela)
    n / N              próxima / anterior ocorrência
    m                  alterna entre renderizado e fonte crua
    v                  (JSON) alterna tabela / esquema / texto
    r                  recarrega o arquivo
    q, Esc             sai
";

enum Source {
    File(PathBuf),
    Stdin,
}

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
    let latest_dir = if cli.latest {
        let dir = cli
            .path
            .as_deref()
            .ok_or_else(|| Failure::Usage("a opção --latest precisa de uma pasta".to_string()))?;
        Some(PathBuf::from(dir))
    } else {
        None
    };
    let stdout_tty = io::stdout().is_terminal();
    let interactive = !cli.plain && stdout_tty;
    let width = cli.width.unwrap_or(DEFAULT_WIDTH);

    // `--follow` com stdin: a TUI abre na hora e o texto vai chegando aos poucos.
    if cli.follow && interactive && !cli.toc && latest_dir.is_none() && reads_stdin(cli.path.as_deref()) {
        let rx = stream::spawn_stdin_reader(usize::try_from(MAX_INPUT_BYTES).unwrap_or(usize::MAX));
        return run_tui(App::new(Source::Stdin, String::new(), width, &cli).with_stream(rx));
    }

    let (source, md) = match &latest_dir {
        Some(dir) => load_latest(dir)?,
        None => load_source(cli.path.as_deref())?,
    };
    if cli.toc && cli.mode == cli::Mode::Markdown {
        return write_stdout(&render::toc(&render::render(&md, width)));
    }
    if interactive {
        let mut app = App::new(source, md, width, &cli);
        if let Some(dir) = latest_dir {
            app = app.with_latest(dir);
        }
        run_tui(app)
    } else {
        run_plain(&md, &source, cli.width, stdout_tty, &cli)
    }
}

/// O documento vem de stdin (`-` ou entrada redirecionada sem arquivo)?
fn reads_stdin(path: Option<&str>) -> bool {
    match path {
        Some(p) => p == "-",
        None => !io::stdin().is_terminal(),
    }
}

/// Abre o `.md` mais recente de `dir` (`--latest`).
fn load_latest(dir: &Path) -> Result<(Source, String), Failure> {
    if !dir.is_dir() {
        return Err(Failure::Runtime(format!("{} não é uma pasta", dir.display())));
    }
    let newest = stream::newest_md(dir)
        .ok_or_else(|| Failure::Runtime(format!("nenhum arquivo .md em {}", dir.display())))?;
    let md = read_markdown_file(&newest).map_err(read_failure(&newest.display().to_string()))?;
    Ok((Source::File(newest), md))
}

/// Lê o documento do arquivo informado, de stdin (`-` ou entrada redirecionada).
fn load_source(path: Option<&str>) -> Result<(Source, String), Failure> {
    match path {
        Some("-") => Ok((Source::Stdin, read_stdin().map_err(read_failure("stdin"))?)),
        Some(p) => {
            let md = read_markdown_file(Path::new(p)).map_err(read_failure(p))?;
            Ok((Source::File(PathBuf::from(p)), md))
        }
        None if !io::stdin().is_terminal() => {
            Ok((Source::Stdin, read_stdin().map_err(read_failure("stdin"))?))
        }
        None => Err(Failure::Usage("nenhum arquivo informado".to_string())),
    }
}

/// Imagens relativas são resolvidas a partir da pasta do arquivo (ou do diretório atual, com stdin).
/// `--remote` baixa imagens de URLs e `--mermaid` renderiza diagramas (`blocking`: espera, como no `--plain`).
fn image_store(source: &Source, cli: &cli::Cli, blocking: bool) -> image::ImageStore {
    let base = match source {
        Source::File(p) => p
            .parent()
            .map(|d| if d.as_os_str().is_empty() { PathBuf::from(".") } else { d.to_path_buf() }),
        Source::Stdin => None,
    };
    let mut store = image::ImageStore::new(base);
    if cli.remote {
        store = store.with_remote(net::FetchPolicy::default(), blocking);
    }
    if cli.mermaid {
        let bin = std::env::var("MDV_MMDC").unwrap_or_else(|_| "mmdc".to_string());
        store = store.with_mermaid(bin, blocking);
    }
    store
}

/// Como a entrada é interpretada: o modo e as opções que só valem para JSON ou tabela.
#[derive(Clone)]
struct DocOpts {
    mode: cli::Mode,
    json_path: Option<String>,
    json_view: cli::JsonView,
    head: usize,
    cols: Vec<String>,
    no_header: bool,
}

impl DocOpts {
    fn from_cli(cli: &cli::Cli) -> Self {
        DocOpts {
            mode: cli.mode,
            json_path: cli.json_path.clone(),
            json_view: cli.json_view,
            head: cli.head,
            cols: cli.cols.clone(),
            no_header: cli.no_header,
        }
    }
}

/// Único ponto que decide como o texto da entrada vira linhas, conforme o modo (TUI e `--plain`).
fn render_doc(doc: &DocOpts, text: &str, width: usize, images: &image::ImageStore) -> render::Rendered {
    match doc.mode {
        cli::Mode::Markdown => {
            render::render_with(text, &render::RenderOpts { width, images: Some(images) })
        }
        cli::Mode::Json => {
            let opts = data::JsonOpts {
                path: doc.json_path.clone(),
                view: doc.json_view,
                head: doc.head,
                cols: doc.cols.clone(),
            };
            data::render_json(text, &opts, width)
        }
        cli::Mode::Table { delimiter } => {
            let opts = data::TableOpts {
                delimiter,
                head: doc.head,
                cols: doc.cols.clone(),
                no_header: doc.no_header,
            };
            data::render_table_data(text, &opts, width)
        }
    }
}

/// Linguagem usada para mostrar a fonte crua (`m`).
fn raw_lang(mode: cli::Mode) -> &'static str {
    match mode {
        cli::Mode::Markdown => "md",
        cli::Mode::Json => "json",
        cli::Mode::Table { .. } => "text",
    }
}

/// Imprime o documento renderizado (com cores só se a saída for um terminal).
fn run_plain(md: &str, source: &Source, width: Option<usize>, color: bool, cli: &cli::Cli) -> Result<(), Failure> {
    let width = width.unwrap_or_else(|| {
        terminal::size()
            .map_or(FALLBACK_TERMINAL_WIDTH, |(cols, _)| usize::from(cols))
            .min(DEFAULT_WIDTH)
    });
    let images = image_store(source, cli, true);
    let out = render_doc(&DocOpts::from_cli(cli), md, width, &images);
    write_stdout(&render::to_ansi(&out.lines, color))
}

fn run_tui(mut app: App) -> Result<(), Failure> {
    let mut term = ratatui::init();
    let res = event_loop(&mut term, &mut app);
    ratatui::restore();
    res.map_err(|e| Failure::Runtime(format!("erro no terminal: {e}")))
}

fn write_stdout(text: &str) -> Result<(), Failure> {
    write_tolerating_closed_pipe(io::stdout().lock(), text.as_bytes())
        .map_err(|e| Failure::Runtime(format!("não consegui escrever a saída: {e}")))
}

/// Escreve tudo em `out`. Se o leitor fechou o pipe (`mdv -p x | head`), a saída
/// foi simplesmente encerrada: isso não é erro.
fn write_tolerating_closed_pipe(mut out: impl Write, bytes: &[u8]) -> io::Result<()> {
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

fn read_failure(what: &str) -> impl FnOnce(io::Error) -> Failure + '_ {
    move |e| Failure::Runtime(format!("não consegui ler {what}: {e}"))
}

fn read_stdin() -> io::Result<String> {
    read_limited(io::stdin().lock(), MAX_INPUT_BYTES)
}

/// Lê um arquivo regular (nunca dispositivo, FIFO ou diretório) respeitando o teto.
fn read_markdown_file(path: &Path) -> io::Result<String> {
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "não é um arquivo regular",
        ));
    }
    read_limited(fs::File::open(path)?, MAX_INPUT_BYTES)
}

/// Lê UTF-8 de `reader`, falhando se passar de `max` bytes (sem ler além de `max + 1`).
fn read_limited(reader: impl Read, max: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        let msg = format!("entrada maior que {} MiB", max / (1024 * 1024));
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn file_mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn event_loop(term: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        term.draw(|f| app.draw(f))?;
        let interval = if app.stream.is_some() { STREAM_POLL_INTERVAL } else { POLL_INTERVAL };
        if event::poll(interval)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && app.on_key(k) {
                    return Ok(());
                }
            }
        } else {
            app.drain_stream();
            app.check_reload();
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
        let q = self.query.to_lowercase();
        for (i, l) in lines.iter().enumerate() {
            if line_text(l).to_lowercase().contains(&q) {
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
    doc: DocOpts,
    stream: Option<Receiver<String>>,
    stick_bottom: bool,
    latest_dir: Option<PathBuf>,
    name: String,
    md: String,
    mtime: Option<SystemTime>,
    max_width: usize,
    rendered: render::Rendered,
    render_width: usize,
    dirty: bool,
    view: Viewport,
    search: SearchState,
    zoom: Option<Zoom>,
    notice: Option<String>,
}

impl App {
    fn new(source: Source, md: String, max_width: usize, cli: &cli::Cli) -> Self {
        let (name, mtime) = match &source {
            Source::File(p) => (
                p.file_name()
                    .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
                file_mtime(p),
            ),
            Source::Stdin => ("stdin".to_string(), None),
        };
        App {
            images: image_store(&source, cli, false),
            seen_images_version: 0,
            raw: false,
            doc: DocOpts::from_cli(cli),
            stream: None,
            stick_bottom: false,
            latest_dir: None,
            source,
            name,
            md,
            mtime,
            max_width: render::clamp_width(max_width),
            rendered: render::Rendered::default(),
            render_width: 0,
            dirty: true,
            view: Viewport::default(),
            search: SearchState::default(),
            zoom: None,
            notice: None,
        }
    }

    /// Passa a receber o texto aos poucos (`--follow`).
    fn with_stream(mut self, rx: Receiver<String>) -> Self {
        self.stream = Some(rx);
        // Com o texto ainda chegando, um bloco mermaid pode estar incompleto: só gera no fim.
        self.images.set_mermaid_hold(true);
        self
    }

    /// Passa a seguir o `.md` mais novo de `dir` (`--latest`).
    fn with_latest(mut self, dir: PathBuf) -> Self {
        self.latest_dir = Some(dir);
        self
    }

    /// Acrescenta o que chegou do stdin; só passa a acompanhar o fim se o leitor já estava nele.
    fn drain_stream(&mut self) {
        let Some(rx) = &self.stream else { return };
        let was_at_bottom = self.view.scroll >= self.view.max_scroll(self.rendered.lines.len());
        let mut got = false;
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(chunk) => {
                    self.md.push_str(&chunk);
                    got = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.stream = None;
            self.images.set_mermaid_hold(false);
            got = true;
        }
        if got {
            self.stick_bottom |= was_at_bottom;
            self.dirty = true;
        }
    }

    /// Depois de redesenhar, leva a tela ao fim se o leitor estava acompanhando o fim.
    fn apply_stick_bottom(&mut self) {
        if self.stick_bottom {
            self.view.scroll = self.view.max_scroll(self.total_lines());
            self.stick_bottom = false;
        }
    }

    /// Se há um `.md` mais novo na pasta acompanhada, troca para ele.
    fn follow_latest(&mut self) {
        let Some(dir) = &self.latest_dir else { return };
        let Some(newest) = stream::newest_md(dir) else { return };
        if matches!(&self.source, Source::File(p) if *p == newest) {
            return;
        }
        let Ok(md) = read_markdown_file(&newest) else { return };
        self.name = newest
            .file_name()
            .map_or_else(|| newest.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.mtime = file_mtime(&newest);
        self.source = Source::File(newest);
        self.md = md;
        self.dirty = true;
        self.view.scroll = 0;
    }

    fn total_lines(&self) -> usize {
        self.rendered.lines.len()
    }

    fn scroll_by(&mut self, delta: isize) {
        self.view.scroll_by(delta, self.total_lines());
    }

    fn scroll_to(&mut self, line: usize) {
        self.view.scroll_to(line, self.total_lines());
    }

    fn rerender(&mut self, w: usize) {
        self.rendered = if self.raw {
            render::render_code_only(&self.md, raw_lang(self.doc.mode), w)
        } else {
            render_doc(&self.doc, &self.md, w, &self.images)
        };
        self.render_width = w;
        self.dirty = false;
        self.search.refresh(&self.rendered.lines);
        let n = self.rendered.tables.len();
        if self.zoom.as_ref().is_some_and(|z| z.table >= n) {
            self.zoom = None;
        } else if let Some(z) = &mut self.zoom {
            z.drawn = None;
        }
    }

    /// Recarrega só se o arquivo mudou desde a última leitura.
    fn check_reload(&mut self) {
        // Download de imagem terminou em segundo plano: redesenha com ela.
        let version = self.images.version();
        if version != self.seen_images_version {
            self.seen_images_version = version;
            self.dirty = true;
        }
        self.follow_latest();
        let Source::File(p) = &self.source else { return };
        let mtime = file_mtime(p);
        if mtime.is_some() && mtime != self.mtime {
            self.reload();
        }
    }

    /// Relê o arquivo; se a leitura falhar, mantém o documento atual.
    fn reload(&mut self) {
        let Source::File(p) = &self.source else { return };
        // Mtime lido antes do conteúdo: uma escrita no meio da leitura é detectada na próxima volta.
        let mtime = file_mtime(p);
        if let Ok(md) = read_markdown_file(p) {
            self.md = md;
            self.mtime = mtime;
            self.dirty = true;
        }
    }

    fn set_search(&mut self, query: String) {
        self.search.commit(query, &self.rendered.lines, self.view.scroll);
        self.goto_match();
    }

        fn open_zoom(&mut self, table: usize) {
            if table >= self.rendered.tables.len() {
                self.notice = Some(format!(
                    "tabela {} não existe (há {})",
                    table + 1,
                    self.rendered.tables.len()
                ));
                return;
            }
            self.notice = None;
            self.zoom = Some(Zoom::new(table));
        }

        /// Abre o zoom da primeira tabela que aparece na tela.
        fn open_visible_table(&mut self) {
            let (top, bottom) = (self.view.scroll, self.view.scroll + self.view.height);
            match self.rendered.tables.iter().position(|t| t.end > top && t.start < bottom) {
                Some(i) => self.open_zoom(i),
                None => self.notice = Some("nenhuma tabela na tela".to_string()),
            }
        }

        /// Redesenha a tabela do zoom se a largura ou a coluna mudaram.
        fn refresh_zoom(&mut self, width: usize) {
            let Some(z) = self.zoom.as_mut() else { return };
            let Some(span) = self.rendered.tables.get(z.table) else { return };
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
                _ => {}
            }
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
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return true;
        }
                if self.zoom.is_some() {
                    self.on_zoom_key(k.code);
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
                if self.search.query.is_empty() {
                    return true;
                }
                self.search.clear();
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('d') => self.scroll_by(half),
            KeyCode::Char('u') => self.scroll_by(-half),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(0),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(usize::MAX),
            KeyCode::Char(']') => {
                let scroll = self.view.scroll;
                if let Some(&h) = self.rendered.headings.iter().find(|&&h| h > scroll) {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('[') => {
                let scroll = self.view.scroll;
                if let Some(&h) = self.rendered.headings.iter().rev().find(|&&h| h < scroll) {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('/') => self.search.input = Some(String::new()),
            KeyCode::Char('n') => self.step_match(true),
            KeyCode::Char('N') => self.step_match(false),
            KeyCode::Char('r') => self.reload(),
                        KeyCode::Char('v') if self.doc.mode == cli::Mode::Json => {
                            let current = if self.doc.json_view == cli::JsonView::Auto {
                                data::auto_view_of_text(&self.md, self.doc.json_path.as_deref())
                            } else {
                                self.doc.json_view
                            };
                            self.doc.json_view = data::next_view(current);
                            self.dirty = true;
                        }
                        KeyCode::Char('t') => self.open_visible_table(),
            KeyCode::Char('m') => {
                self.raw = !self.raw;
                self.dirty = true;
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

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let [content, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let w = usize::from(content.width)
            .saturating_sub(TUI_MARGIN)
            .min(self.max_width)
            .max(MIN_TUI_WIDTH);
        if self.dirty || w != self.render_width {
            self.rerender(w);
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
        let start = self.view.scroll;
        let end = (start + self.view.height).min(self.total_lines());
        let q = self.search.query.to_lowercase();
        let visible: Vec<Line<'static>> = self.rendered.lines[start..end]
            .iter()
            .map(|l| if q.is_empty() { l.clone() } else { highlight_line(l, &q) })
            .collect();
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
        let right = "j/k rolar  [ ] pular título  / buscar  t tabela  m fonte  q sair ";
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
            f.render_widget(Paragraph::new(visible), content);

            let bar = Style::default().bg(Color::Rgb(59, 66, 82)).fg(Color::Rgb(229, 233, 240));
            let left = format!(
                " {}  tabela {}/{}  {}",
                self.name,
                z.table + 1,
                self.rendered.tables.len(),
                columns_label(tv)
            );
            let right = "j/k rolar  h/l colunas  0/$ extremos  Esc voltar ";
            let width = usize::from(status.width);
            let (lw, rw) = (left.width(), right.width());
            let text = if lw + rw < width { format!("{left}{}{right}", " ".repeat(width - lw - rw)) } else { left };
            f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
        }

    fn status_left(&self) -> String {
        let total = self.total_lines();
        let pct = self
            .view
            .scroll
            .saturating_mul(100)
            .checked_div(self.view.max_scroll(total))
            .unwrap_or(100);
                let view = match (self.doc.mode, self.doc.json_view) {
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
                if let Some(n) = &self.notice {
                    let _ = write!(s, "  {n}");
                }
        s
    }
}

fn line_text(l: &Line<'_>) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Destaca as ocorrências da busca (`q` já em minúsculas) dentro de cada trecho da linha.
fn highlight_line(line: &Line<'static>, q: &str) -> Line<'static> {
    if q.is_empty() {
        return line.clone();
    }
    let hl = Style::default().bg(Color::Rgb(255, 213, 79)).fg(Color::Black);
    let mut out: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        let text = span.content.as_ref();
        let lower = text.to_lowercase();
        if lower.len() != text.len() || !lower.contains(q) {
            out.push(span.clone());
            continue;
        }
        let mut start = 0;
        let mut ok = true;
        let mut parts: Vec<Span<'static>> = Vec::new();
        while let Some(pos) = lower[start..].find(q) {
            let s = start + pos;
            let e = s + q.len();
            if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
                ok = false;
                break;
            }
            if s > start {
                parts.push(Span::styled(text[start..s].to_string(), span.style));
            }
            parts.push(Span::styled(text[s..e].to_string(), span.style.patch(hl)));
            start = e;
        }
        if !ok {
            out.push(span.clone());
            continue;
        }
        if start < text.len() {
            parts.push(Span::styled(text[start..].to_string(), span.style));
        }
        out.extend(parts);
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn read_limited_aceita_exatamente_o_teto() {
        let text = read_limited(&b"abcd"[..], 4).unwrap();
        assert_eq!(text, "abcd");
    }

    #[test]
    fn read_limited_rejeita_acima_do_teto() {
        let err = read_limited(&b"abcde"[..], 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_limited_nao_le_alem_do_teto() {
        let mut infinite = io::repeat(b'a');
        assert!(read_limited(&mut infinite, 1024).is_err());
    }

    #[test]
    fn read_limited_rejeita_utf8_invalido() {
        let err = read_limited(&[0xff, 0xfe][..], 16).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_markdown_file_rejeita_diretorio() {
        let err = read_markdown_file(Path::new(".")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn read_markdown_file_rejeita_dispositivo() {
        let err = read_markdown_file(Path::new("/dev/zero")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn pipe_quebrado_na_escrita_e_saida_normal() {
        assert!(write_tolerating_closed_pipe(FailingWriter(io::ErrorKind::BrokenPipe), b"x").is_ok());
    }

    #[test]
    fn outros_erros_de_escrita_continuam_erros() {
        let err = write_tolerating_closed_pipe(FailingWriter(io::ErrorKind::PermissionDenied), b"x");
        assert_eq!(err.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
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

    fn test_app(source: Source, md: &str) -> App {
        let cli = match cli::parse(["x.md".to_string()]) {
            Ok(cli::Parsed::Run(c)) => c,
            _ => panic!("cli de teste"),
        };
        App::new(source, md.to_string(), 80, &cli)
    }

    #[test]
    fn stream_acumula_pedacos_e_marca_para_redesenhar() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        app.dirty = false;
        tx.send("# um\n".to_string()).unwrap();
        tx.send("dois".to_string()).unwrap();
        app.drain_stream();
        assert_eq!(app.md, "# um\ndois");
        assert!(app.dirty);
    }

    #[test]
    fn diagramas_esperam_o_fim_do_stdin_em_streaming() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        assert!(app.images.mermaid_held(), "enquanto o texto chega, não dispara o mmdc");
        tx.send("# x\n".to_string()).unwrap();
        app.drain_stream();
        assert!(app.images.mermaid_held());
        drop(tx);
        app.dirty = false;
        app.drain_stream();
        assert!(!app.images.mermaid_held(), "entrada terminou: libera os diagramas");
        assert!(app.dirty, "redesenha para os diagramas aparecerem");
    }

    #[test]
    fn barra_de_status_explica_que_as_teclas_pulam_de_titulo() {
        let app = test_app(Source::Stdin, "# a");
        let barra: String = app.status_line(120).spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(barra.contains("[ ] pular título"), "{barra}");
    }

    #[test]
    fn stream_sem_novidade_nao_redesenha() {
        let (_tx, rx) = std::sync::mpsc::channel::<String>();
        let mut app = test_app(Source::Stdin, "").with_stream(rx);
        app.dirty = false;
        app.drain_stream();
        assert!(!app.dirty);
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

    #[test]
    fn latest_troca_para_o_arquivo_mais_novo() {
        use std::time::{Duration, SystemTime};
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.md"), dir.path().join("b.md"));
        fs::write(&a, "# a").unwrap();
        let now = SystemTime::now();
        fs::File::options().write(true).open(&a).unwrap().set_modified(now - Duration::from_secs(60)).unwrap();
        let mut app = test_app(Source::File(a), "# a").with_latest(dir.path().to_path_buf());
        fs::write(&b, "# b").unwrap();
        app.check_reload();
        assert_eq!(app.md, "# b");
        assert_eq!(app.name, "b.md");
        assert!(matches!(&app.source, Source::File(p) if *p == b));
        assert!(app.dirty);
    }

    #[test]
    fn latest_sem_novidade_mantem_o_arquivo() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        fs::write(&a, "# a").unwrap();
        let mut app = test_app(Source::File(a), "# a").with_latest(dir.path().to_path_buf());
        app.dirty = false;
        app.check_reload();
        assert_eq!(app.name, "a.md");
        assert!(!app.dirty);
    }

        fn tecla(app: &mut App, c: char) {
            app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        #[test]
        fn v_alterna_as_vistas_do_json() {
            let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
            app.doc.mode = cli::Mode::Json;
            tecla(&mut app, 'v'); // Auto já era tabela: o próximo é o esquema
            assert_eq!(app.doc.json_view, cli::JsonView::Schema);
            assert!(app.dirty);
            tecla(&mut app, 'v');
            assert_eq!(app.doc.json_view, cli::JsonView::Text);
            tecla(&mut app, 'v');
            assert_eq!(app.doc.json_view, cli::JsonView::Table);
        }

        #[test]
        fn v_em_json_que_nao_e_lista_comeca_pela_tabela() {
            let mut app = test_app(Source::Stdin, r#"{"a":1}"#);
            app.doc.mode = cli::Mode::Json;
            tecla(&mut app, 'v');
            assert_eq!(app.doc.json_view, cli::JsonView::Table);
        }

        #[test]
        fn v_fora_do_modo_json_nao_faz_nada() {
            let mut app = test_app(Source::Stdin, "# a");
            app.dirty = false;
            tecla(&mut app, 'v');
            assert_eq!(app.doc.json_view, cli::JsonView::Auto);
            assert!(!app.dirty);
        }

        #[test]
        fn barra_mostra_a_vista_do_json_quando_escolhida() {
            let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
            app.doc.mode = cli::Mode::Json;
            tecla(&mut app, 'v');
            assert!(app.status_left().contains("[esquema]"), "{}", app.status_left());
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

        const TABELA: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";

        fn tabela_larga(n: usize) -> String {
            let h: String = (1..=n).map(|i| format!(" c{i} |")).collect();
            format!("|{h}\n|{}\n|{h}\n", "---|".repeat(n))
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
            let segunda = app.rendered.tables[1].start;
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
            app.md = "sem tabela\n".to_string();
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
}
