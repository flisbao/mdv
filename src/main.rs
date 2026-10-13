mod cli;
mod image;
mod mermaid;
mod net;
mod render;

use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
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

const HELP: &str = "\
mdv - visualizador de Markdown no terminal

USO:
    mdv [opções] <arquivo.md>
    cat arquivo.md | mdv

OPÇÕES:
    -p, --plain        imprime no terminal (sem TUI), como `cat` com cores
    -w, --width <N>    largura máxima do texto (padrão: 100)
        --toc          imprime só o índice de títulos
    -h, --help         mostra esta ajuda
    -V, --version      mostra a versão

TECLAS (modo interativo):
    j / k, ↓ / ↑       rola uma linha
    espaço / b         página abaixo / acima
    d / u              meia página
    g / G              início / fim
    ] / [              próximo / título anterior
    /                  buscar (Enter confirma, Esc cancela)
    n / N              próxima / anterior ocorrência
    m                  alterna entre renderizado e fonte crua
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
    let (source, md) = load_source(cli.path.as_deref())?;
    if cli.toc && cli.mode == cli::Mode::Markdown {
        let width = cli.width.unwrap_or(DEFAULT_WIDTH);
        return write_stdout(&render::toc(&render::render(&md, width)));
    }
    let stdout_tty = io::stdout().is_terminal();
    if cli.plain || !stdout_tty {
        run_plain(&md, &source, cli.width, stdout_tty, &cli)
    } else {
        run_tui(source, md, cli.width.unwrap_or(DEFAULT_WIDTH), &cli)
    }
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

/// Imprime o documento renderizado (com cores só se a saída for um terminal).
fn run_plain(md: &str, source: &Source, width: Option<usize>, color: bool, cli: &cli::Cli) -> Result<(), Failure> {
    let width = width.unwrap_or_else(|| {
        terminal::size()
            .map_or(FALLBACK_TERMINAL_WIDTH, |(cols, _)| usize::from(cols))
            .min(DEFAULT_WIDTH)
    });
    let images = image_store(source, cli, true);
    let out = render::render_with(md, &render::RenderOpts { width, images: Some(&images) });
    write_stdout(&render::to_ansi(&out.lines, color))
}

fn run_tui(source: Source, md: String, width: usize, cli: &cli::Cli) -> Result<(), Failure> {
    let mut app = App::new(source, md, width, cli);
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
        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && app.on_key(k) {
                    return Ok(());
                }
            }
        } else {
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
    name: String,
    md: String,
    mtime: Option<SystemTime>,
    max_width: usize,
    rendered: render::Rendered,
    render_width: usize,
    dirty: bool,
    view: Viewport,
    search: SearchState,
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
        }
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
            render::render_code_only(&self.md, "md", w)
        } else {
            render::render_with(&self.md, &render::RenderOpts { width: w, images: Some(&self.images) })
        };
        self.render_width = w;
        self.dirty = false;
        self.search.refresh(&self.rendered.lines);
    }

    /// Recarrega só se o arquivo mudou desde a última leitura.
    fn check_reload(&mut self) {
        // Download de imagem terminou em segundo plano: redesenha com ela.
        let version = self.images.version();
        if version != self.seen_images_version {
            self.seen_images_version = version;
            self.dirty = true;
        }
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
        self.view.height = usize::from(content.height);
        self.view.scroll = self.view.scroll.min(self.view.max_scroll(self.total_lines()));

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
        let right = "j/k rolar  [ ] títulos  / buscar  m fonte  q sair ";
        let (lw, rw) = (left.width(), right.width());
        let text = if lw + rw < width {
            format!("{left}{}{right}", " ".repeat(width - lw - rw))
        } else {
            left
        };
        Line::from(text)
    }

    /// Nome do arquivo, posição no documento e estado da busca.
    fn status_left(&self) -> String {
        let total = self.total_lines();
        let pct = self
            .view
            .scroll
            .saturating_mul(100)
            .checked_div(self.view.max_scroll(total))
            .unwrap_or(100);
        let mut s = format!(" {}  {pct}%{}", self.name, if self.raw { "  [fonte]" } else { "" });
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
}
