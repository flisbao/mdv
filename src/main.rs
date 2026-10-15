mod cli;
mod clip;
mod data;
mod diff;
mod extract;
mod fold;
mod graphics;
mod image;
mod mem;
mod mermaid;
mod net;
mod stream;
mod render;

use std::collections::{BTreeSet, HashMap, HashSet};
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

#[global_allocator]
static ALLOC: mem::CountingAlloc = mem::CountingAlloc;
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
        --table <N>    abre direto no zoom da N-ésima tabela (só no modo interativo)
        --memory-buffer <T>  limite de memória (padrão: 512M; ex.: 256M, 2G; ou $MDV_MEMORY_BUFFER)
        --extract-code imprime só o texto dos blocos de código (sem cor)
        --lang <L>     com --extract-code: só os blocos da linguagem L
    -n <N>             com --extract-code: só o N-ésimo bloco (a partir de 1)
        --section <T>  imprime só o Markdown da seção de título T
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
    w                  código sem quebra: h/l (ou ←/→) rolam 8 colunas, 0/$ início/fim; w de novo volta a quebrar
    v                  (JSON) alterna tabela / esquema / texto
    t                  zoom da tabela na tela (h/l colunas, Esc volta)
    } / {              próxima / anterior mudança marcada no reload; c limpa as marcas
    z / Z              dobra ou abre a seção atual / dobra todas (ou abre todas); título dobrado mostra ▸ N linhas
    y / Y              copia o bloco de código da tela / o Markdown da seção (OSC 52, até 64 KiB)
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
    let limit = mem::resolve(cli.memory_buffer, std::env::var("MDV_MEMORY_BUFFER").ok().as_deref())
        .map_err(Failure::Usage)?;
    mem::set_limit(limit);
    mem::init();
    let mem_warning = mem::warn_if_above(limit, mem::available());
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
    if !interactive {
        if let Some(w) = &mem_warning {
            eprintln!("mdv: {w}");
        }
    }

    // `--follow` com stdin: a TUI abre na hora e o texto vai chegando aos poucos.
    if cli.follow && interactive && !cli.toc && latest_dir.is_none() && reads_stdin(cli.path.as_deref()) {
        let rx = stream::spawn_stdin_reader(usize::try_from(MAX_INPUT_BYTES).unwrap_or(usize::MAX));
        let mut app = App::new(Source::Stdin, String::new(), width, &cli).with_stream(rx);
        app.notice = app.notice.or(mem_warning);
        return run_tui(app);
    }

    let (source, md) = match &latest_dir {
        Some(dir) => load_latest(dir)?,
        None => load_source(cli.path.as_deref())?,
    };
    if cli.wants_extract() {
        mem::check_parse(md.len()).map_err(|e| Failure::Runtime(e.to_string()))?;
        let opts = extract::ExtractOpts {
            code: cli.extract_code,
            lang: cli.lang.clone(),
            nth: cli.nth,
            section: cli.section.clone(),
        };
        let text = extract::extract(&md, &opts).map_err(Failure::Runtime)?;
        return write_stdout(&text);
    }
    if cli.toc && cli.mode == cli::Mode::Markdown {
        return write_stdout(&render::toc(&render::render(&md, width)));
    }
    if interactive {
        let mut app = App::new(source, md, width, &cli);
        app.notice = app.notice.or(mem_warning);
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
    code_scroll: Option<usize>,
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
            code_scroll: None,
        }
    }
}

/// Único ponto que decide como o texto da entrada vira linhas, conforme o modo (TUI e `--plain`).
fn render_doc(doc: &DocOpts, text: &str, width: usize, images: &image::ImageStore) -> render::Rendered {
    match doc.mode {
        cli::Mode::Markdown => {
            render::render_with(
                text,
                &render::RenderOpts { width, images: Some(images), code_scroll: doc.code_scroll },
            )
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
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "não é um arquivo regular",
        ));
    }
    mem::check_input(meta.len())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
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

/// Escreve uma sequência de controle direto no terminal (OSC 52).
fn emit_osc(seq: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
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
                if let Some(seq) = app.pending_osc.take() {
                    emit_osc(&seq);
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
    open_table: Option<usize>,
    changed: BTreeSet<usize>,
    marks: Vec<bool>,
    folded: HashSet<fold::FoldKey>,
    display: Vec<usize>,
    fold_hidden: HashMap<usize, usize>,
    pending_osc: Option<String>,
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
            changed: BTreeSet::new(),
            marks: Vec::new(),
            folded: HashSet::new(),
            display: Vec::new(),
            fold_hidden: HashMap::new(),
            notice: None,
            open_table: cli.table.map(|n| n - 1),
            pending_osc: None,
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
            self.append_md(&chunks);
        }
        self.stop_stream_for_memory(mem::check().err());
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

    /// Para de acumular o stdin quando a memória passou do limite (`--follow`).
    fn stop_stream_for_memory(&mut self, over: Option<mem::MemoryLimit>) {
        if let Some(e) = over {
            if self.stream.take().is_some() {
                self.images.set_mermaid_hold(false);
                self.notice = Some(format!("leitura interrompida: {e}"));
                self.dirty = true;
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
        self.changed.clear();
        self.marks.clear();
        self.folded.clear();
        self.dirty = true;
        self.view.scroll = 0;
    }

    fn total_lines(&self) -> usize {
        self.display.len()
    }

    /// Recalcula quais linhas aparecem (depois de renderizar ou de dobrar).
    fn recompute_display(&mut self) {
        let d = fold::compute(&self.rendered, &self.folded);
        self.display = d.lines;
        self.fold_hidden = d.hidden;
        self.view.scroll = self.view.scroll.min(self.view.max_scroll(self.display.len()));
    }

    /// Posição de `display` → linha renderizada; além do fim, o total de linhas.
    fn line_at(&self, pos: usize) -> usize {
        self.display.get(pos).copied().unwrap_or(self.rendered.lines.len())
    }

    /// Primeira posição de `display` cuja linha é `line` ou vem depois.
    fn pos_of(&self, line: usize) -> usize {
        self.display.partition_point(|&l| l < line)
    }

    fn is_visible(&self, line: usize) -> bool {
        self.display.binary_search(&line).is_ok()
    }

    fn scroll_by(&mut self, delta: isize) {
        self.view.scroll_by(delta, self.total_lines());
    }

    fn scroll_to(&mut self, pos: usize) {
        self.view.scroll_to(pos, self.total_lines());
    }

    /// Linha renderizada de referência para `y`/`Y`/`z`/`t`: a primeira visível.
    fn ref_line(&self) -> usize {
        self.line_at(self.view.scroll)
    }

    /// Linhas renderizadas que cabem na janela, já sem as dobradas.
    fn window_display(&self) -> &[usize] {
        let a = self.view.scroll.min(self.display.len());
        let b = (self.view.scroll + self.view.height).min(self.display.len());
        &self.display[a..b]
    }

    /// Rola para que a linha renderizada `line` (ou a próxima visível) fique no topo.
    fn scroll_to_line(&mut self, line: usize) {
        self.scroll_to(self.pos_of(line));
    }

    /// Troca o texto do documento (reload) marcando as linhas novas em relação ao anterior.
    fn set_md(&mut self, new: String) {
        if self.md.is_empty() {
            self.changed.clear();
        } else {
            match diff::changed_lines(&self.md, &new) {
                Some(set) => self.changed = set,
                None => {
                    self.changed.clear();
                    self.notice = Some("documento grande: sem marcação de mudanças".to_string());
                }
            }
        }
        self.md = new;
        self.dirty = true;
    }

    /// Acrescenta um pedaço de stdin (`--follow`) marcando só as linhas que acabaram de chegar.
    fn append_md(&mut self, chunk: &str) {
        let first_new = self.md.matches('\n').count();
        let was_empty = self.md.is_empty();
        self.md.push_str(chunk);
        let total = self.md.matches('\n').count() + usize::from(!self.md.ends_with('\n'));
        if was_empty || total.saturating_sub(first_new) > diff::MAX_DIFF_LINES {
            self.changed.clear();
        } else {
            self.changed = (first_new..total).collect();
        }
    }

    /// Marcas por linha renderizada; linhas em branco nunca são pintadas.
    fn recompute_marks(&mut self) {
        let mut m = diff::marks(&self.rendered.line_src, &self.changed);
        for (i, mark) in m.iter_mut().enumerate() {
            if *mark && self.rendered.lines.get(i).is_none_or(|l| line_text(l).trim().is_empty()) {
                *mark = false;
            }
        }
        self.marks = m;
    }

    fn is_marked(&self, line: usize) -> bool {
        self.marks.get(line).copied().unwrap_or(false)
    }

    /// Quantos trechos contíguos marcados existem.
    fn change_runs(&self) -> usize {
        (0..self.marks.len()).filter(|&i| self.is_marked(i) && (i == 0 || !self.is_marked(i - 1))).count()
    }

    /// A coluna de margem da janela: `▌` amarelo nas linhas marcadas.
    fn margin_lines(&self) -> Vec<Line<'static>> {
        self.window_display()
            .iter()
            .map(|&l| {
                if self.is_marked(l) {
                    Line::from(Span::styled("▌", Style::default().fg(Color::Yellow)))
                } else {
                    Line::from(" ")
                }
            })
            .collect()
    }

    /// `}` / `{`: vai ao início do próximo / anterior trecho marcado.
    fn jump_change(&mut self, forward: bool) {
        if self.change_runs() == 0 {
            self.notice = Some("nenhuma mudança marcada".to_string());
            return;
        }
        let top = self.ref_line();
        let starts = (0..self.marks.len()).filter(|&i| self.is_marked(i) && (i == 0 || !self.is_marked(i - 1)));
        let alvo = if forward { starts.into_iter().find(|&i| i > top) } else { starts.into_iter().filter(|&i| i < top).last() };
        match alvo {
            Some(line) => {
                self.reveal(line);
                self.scroll_to_line(line);
            }
            None => {
                self.notice = Some(if forward { "sem mais mudanças adiante" } else { "sem mais mudanças atrás" }.to_string());
            }
        }
    }

    /// Abre as dobras que escondem `line` (busca, saltos).
    fn reveal(&mut self, line: usize) {
        let n = self.rendered.lines.len();
        let keys = fold::fold_keys(&self.rendered.outline);
        let mut changed = false;
        for (i, key) in keys.iter().enumerate() {
            let (start, end) = render::section_range(&self.rendered.outline, i, n);
            if start < line && line < end {
                changed |= self.folded.remove(key);
            }
        }
        if changed {
            self.recompute_display();
        }
    }

    /// As linhas da janela como serão desenhadas: destaque da busca e sufixo nos títulos dobrados.
    fn screen_lines(&self) -> Vec<Line<'static>> {
        let q = self.search.query.as_str();
        self.window_display()
            .iter()
            .map(|&i| {
                let l = &self.rendered.lines[i];
                let mut l = if q.is_empty() { l.clone() } else { highlight_line(l, q) };
                if let Some(n) = self.fold_hidden.get(&i) {
                    l.spans.push(Span::styled(
                        format!(" ▸ {n} linhas"),
                        Style::default().fg(Color::Rgb(110, 118, 129)),
                    ));
                }
                l
            })
            .collect()
    }

    /// `z`: dobra ou abre a seção do título mais fundo na linha de referência.
    fn toggle_fold(&mut self) {
        let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) else {
            self.notice = Some("nenhum título aqui para dobrar (documento sem títulos?)".to_string());
            return;
        };
        let key = fold::fold_keys(&self.rendered.outline)[idx].clone();
        if !self.folded.remove(&key) {
            self.folded.insert(key);
        }
        let line = self.rendered.outline[idx].line;
        self.recompute_display();
        self.scroll_to_line(line);
    }

    /// `Z`: dobra todas as seções se alguma está aberta; senão abre todas.
    fn toggle_fold_all(&mut self) {
        if self.rendered.outline.is_empty() {
            self.notice = Some("o documento não tem títulos para dobrar".to_string());
            return;
        }
        let keys = fold::fold_keys(&self.rendered.outline);
        let top = self.ref_line();
        // Ancora no título da seção: o corpo some ao dobrar tudo.
        let anchor = render::section_at(&self.rendered.outline, top)
            .map_or(top, |i| self.rendered.outline[i].line);
        if keys.iter().all(|k| self.folded.contains(k)) {
            self.folded.clear();
        } else {
            self.folded.extend(keys);
        }
        self.recompute_display();
        self.scroll_to_line(anchor);
    }

    /// Copia o bloco de código sob a linha de referência (`y`).
    fn copy_code(&mut self) {
        let top = self.ref_line();
        let janela = self.window_display();
        let achado = self
            .rendered
            .code_blocks
            .iter()
            .find(|c| c.start <= top && top < c.end)
            .or_else(|| self.rendered.code_blocks.iter().find(|c| janela.iter().any(|&l| l >= c.start && l < c.end)))
            .map(|c| c.text.clone());
        let Some(text) = achado else {
            self.notice = Some("nenhum bloco de código".to_string());
            return;
        };
        self.copy_text(&text);
    }

    /// Copia o Markdown cru da seção da linha de referência (`Y`).
    fn copy_section(&mut self) {
        if self.raw || self.doc.mode != cli::Mode::Markdown {
            self.notice = Some("seção só existe no Markdown renderizado".to_string());
            return;
        }
        let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) else {
            self.notice = Some("nenhuma seção aqui".to_string());
            return;
        };
        match render::section_source(&self.md, &self.rendered, idx) {
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
        self.rendered = if self.raw {
            render::render_code_only_at(&self.md, raw_lang(self.doc.mode), w, self.doc.code_scroll)
        } else {
            render_doc(&self.doc, &self.md, w, &self.images)
        };
        self.render_width = w;
        self.dirty = false;
        self.search.refresh(&self.rendered.lines);
        self.recompute_display();
        self.recompute_marks();
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
            self.set_md(md);
            self.mtime = mtime;
        }
    }

    fn set_search(&mut self, query: String) {
        self.search.commit(query, &self.rendered.lines, self.ref_line());
        self.goto_match();
    }

            /// Abre o zoom pedido por `--table`, assim que há um documento renderizado.
        fn open_pending_table(&mut self) {
            if let Some(i) = self.open_table.take() {
                self.open_zoom(i);
            }
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
            let janela = self.window_display();
            let achada = self.rendered.tables.iter().position(|t| janela.iter().any(|&l| l >= t.start && l < t.end));
            match achada {
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

    /// `w`: alterna entre código quebrado na largura e código sem quebra, rolável com `h`/`l`.
    fn toggle_code_wrap(&mut self) {
        self.doc.code_scroll = match self.doc.code_scroll {
            None => Some(0),
            Some(_) => None,
        };
        self.dirty = true;
    }

    /// Vai para a coluna `to` do código (limitada ao fim do código mais largo).
    fn set_code_scroll(&mut self, to: usize) {
        let Some(cur) = self.doc.code_scroll else {
            self.notice = Some("w: código sem quebra (h/l rolam)".into());
            return;
        };
        let max = self.rendered.code_overflow;
        let next = to.min(max);
        if next != cur {
            self.doc.code_scroll = Some(next);
            self.dirty = true;
        } else if max == 0 {
            self.notice = Some("nenhum código largo aqui".into());
        }
    }

    fn scroll_code(&mut self, step: usize, forward: bool) {
        let Some(cur) = self.doc.code_scroll else {
            self.set_code_scroll(0);
            return;
        };
        let to = if forward { cur.saturating_add(step) } else { cur.saturating_sub(step) };
        self.set_code_scroll(to);
    }

    fn goto_match(&mut self) {
        if let Some(line) = self.search.current_line() {
            self.reveal(line);
            let pos = self.pos_of(line);
            self.scroll_to(pos.saturating_sub(self.view.height / 3));
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
                let top = self.ref_line();
                let alvo = self.rendered.headings.iter().copied().find(|&h| h > top && self.is_visible(h));
                if let Some(h) = alvo {
                    self.scroll_to_line(h);
                }
            }
            KeyCode::Char('[') => {
                let top = self.ref_line();
                let alvo = self.rendered.headings.iter().rev().copied().find(|&h| h < top && self.is_visible(h));
                if let Some(h) = alvo {
                    self.scroll_to_line(h);
                }
            }
            KeyCode::Char('}') => self.jump_change(true),
            KeyCode::Char('{') => self.jump_change(false),
            KeyCode::Char('c') => {
                self.changed.clear();
                self.marks.clear();
            }
            KeyCode::Char('z') => self.toggle_fold(),
            KeyCode::Char('Z') => self.toggle_fold_all(),
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
                        KeyCode::Char('y') => self.copy_code(),
                        KeyCode::Char('Y') => self.copy_section(),
            KeyCode::Char('w') => self.toggle_code_wrap(),
            KeyCode::Char('l') | KeyCode::Right => self.scroll_code(8, true),
            KeyCode::Char('h') | KeyCode::Left => self.scroll_code(8, false),
            KeyCode::Char('0') => self.set_code_scroll(0),
            KeyCode::Char('$') => self.set_code_scroll(usize::MAX),
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
        self.open_pending_table();
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
        if !self.marks.is_empty() && inner.x > content.x {
            let margem = Rect { x: inner.x - 1, y: inner.y, width: 1, height: inner.height };
            f.render_widget(Paragraph::new(self.margin_lines()), margem);
        }

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
        let right = "j/k rolar  [ ] título  / buscar  z dobrar  t tabela  y/Y copiar  m fonte  q sair ";
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

    /// `☑ feitas/total` do documento e, se a seção atual tem tarefas e difere do todo, `(seção f/t)`.
    fn task_summary(&self) -> Option<String> {
        let all = render::count_tasks(&self.rendered.tasks, 0, usize::MAX);
        if all.total == 0 {
            return None;
        }
        let mut s = format!("☑ {}/{}", all.done, all.total);
        if let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) {
            let c = render::section_tasks(&self.rendered, idx);
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
        if let Some(c) = self.doc.code_scroll {
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
        let runs = self.change_runs();
        if runs > 0 {
            let _ = write!(s, "  ▌ {runs} mudança{} (c limpa)", if runs == 1 { "" } else { "s" });
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
        assert!(barra.contains("[ ] título"), "{barra}");
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

        #[test]
        fn w_liga_codigo_sem_quebra_e_hl_rolam() {
            let longa = "x".repeat(200);
            let mut app = test_app(Source::Stdin, &format!("```\n{longa}\n```"));
            app.rerender(60); // 58 colunas de código => overflow 142
            assert_eq!(app.rendered.code_overflow, 142);
            tecla(&mut app, 'w');
            assert_eq!(app.doc.code_scroll, Some(0));
            assert!(app.dirty);
            app.rerender(60);
            tecla(&mut app, 'l');
            assert_eq!(app.doc.code_scroll, Some(8));
            tecla(&mut app, 'h');
            tecla(&mut app, 'h');
            assert_eq!(app.doc.code_scroll, Some(0));
            tecla(&mut app, '$');
            assert_eq!(app.doc.code_scroll, Some(142));
            tecla(&mut app, '0');
            assert_eq!(app.doc.code_scroll, Some(0));
            tecla(&mut app, 'w');
            assert_eq!(app.doc.code_scroll, None);
        }

        #[test]
        fn hl_sem_w_avisam_e_nao_mudam_nada() {
            let mut app = test_app(Source::Stdin, "```\nabc\n```");
            app.rerender(60);
            tecla(&mut app, 'l');
            assert_eq!(app.doc.code_scroll, None);
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
            assert_eq!(app.doc.code_scroll, Some(12));
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

        const DOC_FOLD: &str = "# A\n\ntexto A\n\n## A1\n\ntexto A1\n\n# B\n\ntexto B\n";

        fn dobrar(app: &mut App, titulo: &str) {
            let keys = fold::fold_keys(&app.rendered.outline);
            let i = app.rendered.outline.iter().position(|e| e.title == titulo).unwrap();
            app.folded.insert(keys[i].clone());
            app.recompute_display();
        }

        #[test]
        fn sem_dobras_display_e_identidade_e_nada_muda() {
            let app = app_com_texto(DOC_FOLD);
            assert_eq!(app.display, (0..app.rendered.lines.len()).collect::<Vec<_>>());
            assert_eq!(app.total_lines(), app.rendered.lines.len());
            assert_eq!(app.line_at(3), 3);
            assert_eq!(app.pos_of(3), 3);
        }

        #[test]
        fn dobra_encurta_o_documento_e_os_mapeamentos_seguem() {
            let mut app = app_com_texto(DOC_FOLD);
            let total = app.rendered.lines.len();
            dobrar(&mut app, "A");
            assert!(app.total_lines() < total);
            let b = app.rendered.outline.iter().find(|e| e.title == "B").unwrap().line;
            assert_eq!(app.line_at(app.pos_of(b)), b, "ida e volta num título visível");
            assert_eq!(app.line_at(usize::MAX), total, "além do fim: o total de linhas");
            assert!(app.is_visible(b) && !app.is_visible(b - 1));
        }

        #[test]
        fn ref_line_e_window_lines_usam_posicoes_do_display() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            let b = app.rendered.outline.iter().find(|e| e.title == "B").unwrap().line;
            app.view.scroll = app.pos_of(b);
            assert_eq!(app.ref_line(), b);
            app.scroll_to_line(0);
            assert_eq!(app.view.scroll, 0);
        }

        #[test]
        fn tela_mostra_o_sufixo_no_titulo_dobrado() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            let texto: Vec<String> = app.screen_lines().iter().map(line_text).collect();
            assert!(texto[0].contains("A") && texto[0].contains("▸") && texto[0].contains("linhas"), "{texto:?}");
            assert!(!texto.iter().any(|l| l.contains("texto A1")));
            assert!(texto.iter().any(|l| l.contains("texto B")));
        }

        #[test]
        fn colchetes_pulam_so_entre_titulos_visiveis() {
            let mut app = app_com_texto(DOC_FOLD);
            app.view.height = 2;
            dobrar(&mut app, "A");
            app.on_key(ev(KeyCode::Char(']')));
            let b = app.rendered.outline.iter().find(|e| e.title == "B").unwrap().line;
            assert_eq!(app.ref_line(), b, "A1 está escondido: o próximo título visível é B");
            app.on_key(ev(KeyCode::Char('[')));
            assert_eq!(app.ref_line(), 0);
        }

        #[test]
        fn busca_numa_secao_dobrada_abre_a_dobra() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.set_search("texto A1".to_string());
            assert!(app.folded.is_empty(), "a dobra que escondia a ocorrência foi aberta");
            let linha = app.search.current_line().unwrap();
            assert!(app.is_visible(linha));
        }

        #[test]
        fn busca_so_abre_a_dobra_que_esconde_a_linha() {
            let mut app = app_com_texto("# A\n\nx\n\n# B\n\ny\n");
            dobrar(&mut app, "A");
            dobrar(&mut app, "B");
            app.set_search("y".to_string());
            assert_eq!(app.folded.len(), 1, "A continua dobrada");
        }

        #[test]
        fn dobras_sobrevivem_a_reload_e_a_nova_largura() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.md = format!("{DOC_FOLD}\nmais um parágrafo\n");
            app.dirty = true;
            app.rerender(40);
            assert!(!app.display.iter().any(|&l| linha_contem(&app, l, "texto A1")));
        }

        #[test]
        fn dobra_de_titulo_que_sumiu_nao_quebra() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.md = "# Outro\n\nx\n".to_string();
            app.rerender(60);
            assert_eq!(app.total_lines(), app.rendered.lines.len());
        }

        fn linha_contem(app: &App, l: usize, trecho: &str) -> bool {
            line_text(&app.rendered.lines[l]).contains(trecho)
        }

        #[test]
        fn t_nao_abre_tabela_escondida_numa_secao_dobrada() {
            let mut app = app_com_texto(&format!("# A\n\n{TABELA}\n# B\n\ntexto\n"));
            dobrar(&mut app, "A");
            app.on_key(ev(KeyCode::Char('t')));
            assert!(app.zoom.is_none());
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma tabela")));
        }

        #[test]
        fn y_nao_copia_bloco_de_codigo_escondido() {
            let mut app = app_com_texto("# A\n\n```\nsegredo\n```\n\n# B\n\ntexto\n");
            dobrar(&mut app, "A");
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(app.pending_osc, None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhum bloco")));
        }

        #[test]
        fn z_dobra_a_secao_da_linha_de_referencia_e_de_novo_abre() {
            let mut app = app_com_texto("# A\n\na\n\n# B\n\nb\n");
            let total = app.total_lines();
            app.on_key(ev(KeyCode::Char('z')));
            assert!(app.total_lines() < total);
            assert_eq!(app.ref_line(), 0, "o título dobrado fica no topo");
            app.on_key(ev(KeyCode::Char('z')));
            assert_eq!(app.total_lines(), total);
            assert!(app.folded.is_empty());
        }

        #[test]
        fn z_age_no_titulo_mais_fundo_da_linha_de_referencia() {
            let mut app = app_com_texto(DOC_FOLD);
            let a1 = app.rendered.outline.iter().find(|e| e.title == "A1").unwrap().line;
            app.view.scroll = app.pos_of(a1);
            app.on_key(ev(KeyCode::Char('z')));
            assert_eq!(app.folded.len(), 1);
            assert!(app.is_visible(0), "o título A continua aberto");
            assert!(!app.display.iter().any(|&l| linha_contem(&app, l, "texto A1")));
            assert!(app.display.iter().any(|&l| linha_contem(&app, l, "texto A")));
        }

        #[test]
        fn z_abre_o_pai_e_o_filho_continua_dobrado() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A1");
            dobrar(&mut app, "A");
            app.view.scroll = 0;
            app.on_key(ev(KeyCode::Char('z'))); // abre A
            assert!(app.display.iter().any(|&l| linha_contem(&app, l, "texto A")));
            assert!(!app.display.iter().any(|&l| linha_contem(&app, l, "texto A1")), "A1 segue dobrado");
        }

        #[test]
        fn maiusculo_dobra_tudo_se_alguma_esta_aberta_e_senao_abre_tudo() {
            let mut app = app_com_texto(DOC_FOLD);
            let total = app.total_lines();
            app.on_key(ev(KeyCode::Char('Z')));
            let dobrado = app.total_lines();
            assert!(dobrado < total);
            assert_eq!(app.folded.len(), 3, "A, A1 e B");
            app.on_key(ev(KeyCode::Char('Z')));
            assert_eq!(app.total_lines(), total);
            assert!(app.folded.is_empty());
        }

        #[test]
        fn maiusculo_com_uma_dobrada_dobra_o_resto() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "B");
            app.on_key(ev(KeyCode::Char('Z')));
            assert_eq!(app.folded.len(), 3);
        }

        #[test]
        fn z_sem_titulos_avisa_sem_quebrar() {
            for md in ["só texto\n", ""] {
                let mut app = app_com_texto(md);
                app.on_key(ev(KeyCode::Char('z')));
                app.on_key(ev(KeyCode::Char('Z')));
                assert!(app.folded.is_empty());
                assert!(app.notice.as_deref().is_some_and(|n| n.contains("títulos")), "{:?}", app.notice);
            }
        }

        #[test]
        fn z_no_modo_fonte_e_em_json_avisa_sem_quebrar() {
            let mut app = app_com_texto("# A\n\nx\n");
            app.on_key(ev(KeyCode::Char('m')));
            app.rerender(60);
            app.on_key(ev(KeyCode::Char('z')));
            assert!(app.folded.is_empty());
            let mut json = test_app(Source::Stdin, r#"[{"a":1}]"#);
            json.doc.mode = cli::Mode::Json;
            json.rerender(60);
            json.view.height = 10;
            json.on_key(ev(KeyCode::Char('Z')));
            assert!(json.folded.is_empty());
        }

        const V1: &str = "primeiro\n\nsegundo\n\nterceiro\n";

        fn linha_com(app: &App, trecho: &str) -> usize {
            (0..app.rendered.lines.len())
                .find(|&i| line_text(&app.rendered.lines[i]).contains(trecho))
                .unwrap_or_else(|| panic!("sem linha com {trecho:?}"))
        }

        fn marcada(app: &App, trecho: &str) -> bool {
            app.marks.get(linha_com(app, trecho)).copied().unwrap_or(false)
        }

        #[test]
        fn carga_inicial_nao_tem_marcas() {
            let app = app_com_texto(V1);
            assert!(app.changed.is_empty());
            assert!(app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn reload_marca_so_as_linhas_novas() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nsegundo EDITADO\n\nterceiro\n\nquarto\n".to_string());
            app.rerender(60);
            assert!(marcada(&app, "EDITADO") && marcada(&app, "quarto"));
            assert!(!marcada(&app, "primeiro") && !marcada(&app, "terceiro"));
        }

        #[test]
        fn linhas_em_branco_entre_blocos_nao_sao_pintadas() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nsegundo\n\nterceiro\n\nA\n\nB\n".to_string());
            app.rerender(60);
            for (i, m) in app.marks.iter().enumerate() {
                if *m {
                    assert!(!line_text(&app.rendered.lines[i]).trim().is_empty(), "linha {i} em branco pintada");
                }
            }
            assert_eq!(app.change_runs(), 2, "A e B são trechos separados pela linha em branco");
        }

        #[test]
        fn reload_duplo_so_marca_a_diferenca_para_a_versao_anterior() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nsegundo\n\nterceiro\n\nquarto\n".to_string());
            app.set_md("primeiro\n\nsegundo\n\nterceiro\n\nquarto\n\nquinto\n".to_string());
            app.rerender(60);
            assert!(marcada(&app, "quinto"));
            assert!(!marcada(&app, "quarto"), "quarto já estava na versão anterior");
        }

        #[test]
        fn reload_sem_diferenca_limpa_as_marcas() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            assert!(!app.changed.is_empty());
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            assert!(app.changed.is_empty() && app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn remocao_nao_marca_nada() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nterceiro\n".to_string());
            app.rerender(60);
            assert!(app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn paragrafo_quebrado_em_varias_linhas_fica_todo_marcado() {
            let longo = "palavra ".repeat(30);
            let mut app = app_com_texto(&format!("{longo}\n\nfim\n"));
            app.set_md(format!("{longo}\n{longo}\n\nfim\n"));
            app.rerender(30);
            let linhas: Vec<usize> = (0..app.rendered.lines.len())
                .filter(|&i| line_text(&app.rendered.lines[i]).contains("palavra"))
                .collect();
            assert!(linhas.len() > 2);
            assert!(linhas.iter().all(|&i| app.marks[i]));
            assert!(!marcada(&app, "fim"));
        }

        #[test]
        fn documento_grande_nao_calcula_diff_e_avisa() {
            let mut app = app_com_texto("a\n");
            app.set_md("x\n".repeat(diff::MAX_DIFF_LINES + 1));
            assert!(app.changed.is_empty());
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("documento grande")), "{:?}", app.notice);
            assert_eq!(app.md.lines().count(), diff::MAX_DIFF_LINES + 1, "o texto novo é adotado mesmo assim");
        }

        #[test]
        fn c_limpa_as_marcas() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            assert!(app.marks.iter().any(|&m| m));
            app.on_key(ev(KeyCode::Char('c')));
            assert!(app.changed.is_empty() && app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn chaves_saltam_entre_trechos_alterados() {
            let base: String = (1..=12).map(|i| format!("par{i}\n\n")).collect();
            let mut app = app_com_texto(&base);
            app.view.height = 3;
            let novo = base.replace("par4\n", "par4 mudou\n").replace("par9\n", "par9 mudou\n");
            app.set_md(novo);
            app.rerender(60);
            app.view.scroll = 0;
            app.on_key(ev(KeyCode::Char('}')));
            assert_eq!(app.ref_line(), linha_com(&app, "par4 mudou"));
            app.on_key(ev(KeyCode::Char('}')));
            assert_eq!(app.ref_line(), linha_com(&app, "par9 mudou"));
            app.on_key(ev(KeyCode::Char('}')));
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("mais")), "{:?}", app.notice);
            app.on_key(ev(KeyCode::Char('{')));
            assert_eq!(app.ref_line(), linha_com(&app, "par4 mudou"));
        }

        #[test]
        fn salto_sem_marcas_avisa() {
            let mut app = app_com_texto(V1);
            app.on_key(ev(KeyCode::Char('}')));
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma mudança")));
        }

        #[test]
        fn margem_desenha_a_barra_amarela_nas_linhas_marcadas() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            let margem = app.margin_lines();
            let i = linha_com(&app, "novo") - app.view.scroll;
            assert_eq!(line_text(&margem[i]), "▌");
            assert_eq!(margem[i].spans[0].style.fg, Some(Color::Yellow));
            assert_eq!(line_text(&margem[0]), " ");
        }

        #[test]
        fn barra_de_status_conta_os_trechos_alterados() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            assert!(app.status_left().contains("1 mudança"), "{}", app.status_left());
        }

        #[test]
        fn recarregar_o_arquivo_marca_o_que_mudou() {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.md");
            std::fs::write(&p, V1).unwrap();
            let mut app = test_app(Source::File(p.clone()), V1);
            app.rerender(60);
            app.view.height = 10;
            std::fs::write(&p, format!("{V1}\nnovo\n")).unwrap();
            app.reload();
            app.rerender(60);
            assert!(marcada(&app, "novo"));
        }

        #[test]
        fn stream_marca_so_o_que_acabou_de_chegar_e_nao_o_primeiro_pedaco() {
            let mut app = test_app(Source::Stdin, "");
            app.append_md("a\n");
            assert!(app.changed.is_empty(), "documento vazio antes: nada de marcar tudo");
            app.append_md("b\nc\n");
            assert_eq!(app.changed.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
            app.append_md("d");
            assert_eq!(app.changed.iter().copied().collect::<Vec<_>>(), vec![3]);
            app.append_md("e\n");
            assert_eq!(app.changed.iter().copied().collect::<Vec<_>>(), vec![3], "a linha parcial foi completada");
        }

        #[test]
        fn maiusculo_dentro_de_uma_secao_fica_no_titulo_dela() {
            let mut app = app_com_texto("# A\n\na\n\n# B\n\nbbb\n\n# C\n\nccc\n\n# D\n\nddd\n");
            app.view.height = 2;
            let corpo = linha_com(&app, "bbb");
            app.view.scroll = app.pos_of(corpo);
            app.on_key(ev(KeyCode::Char('Z')));
            let b = app.rendered.outline[1].line;
            assert_eq!(app.ref_line(), b, "fica em B, não pula para C");
        }

        #[test]
        fn latest_ao_trocar_de_arquivo_zera_as_dobras() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("a.md"), "# Resumo\n\nx\n").unwrap();
            let mut app = test_app(Source::Stdin, "# Resumo\n\nx\n");
            app.rerender(60);
            dobrar(&mut app, "Resumo");
            app.latest_dir = Some(dir.path().to_path_buf());
            app.follow_latest();
            assert!(app.folded.is_empty());
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
            app.view.scroll = app.rendered.outline[1].line;
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
            app.doc.mode = cli::Mode::Json;
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
            let b = app.rendered.outline[1].line;
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
