mod render;

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

const HELP: &str = "\
mdv - visualizador de Markdown no terminal

USO:
    mdv [opções] <arquivo.md>
    cat arquivo.md | mdv

OPÇÕES:
    -p, --plain        imprime no terminal (sem TUI), como `cat` com cores
    -w, --width <N>    largura máxima do texto (padrão: 100)
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
    r                  recarrega o arquivo
    q, Esc             sai
";

enum Source {
    File(PathBuf),
    Stdin,
}

/// Opções da linha de comando para exibir um documento.
#[derive(Debug, PartialEq)]
struct Options {
    plain: bool,
    width: Option<usize>,
    path: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Run(Options),
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

/// Interpreta os argumentos (sem o nome do programa). `--` encerra as opções.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut opts = Options { plain: false, width: None, path: None };
    let mut args = args.into_iter();
    let mut only_paths = false;
    while let Some(a) = args.next() {
        if only_paths || a == "-" || !a.starts_with('-') {
            set_path(&mut opts, a)?;
            continue;
        }
        match a.as_str() {
            "--" => only_paths = true,
            "-p" | "--plain" => opts.plain = true,
            "-w" | "--width" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("a opção {a} precisa de uma largura"))?;
                opts.width = Some(parse_width(&value)?);
            }
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            _ => return Err(format!("opção desconhecida: {a}")),
        }
    }
    Ok(Command::Run(opts))
}

fn set_path(opts: &mut Options, path: String) -> Result<(), String> {
    if opts.path.replace(path).is_some() {
        return Err("informe um único arquivo".to_string());
    }
    Ok(())
}

fn run_cli() -> Result<(), Failure> {
    let opts = match parse_args(std::env::args().skip(1)).map_err(Failure::Usage)? {
        Command::Help => {
            print!("{HELP}");
            return Ok(());
        }
        Command::Version => {
            println!("mdv {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Command::Run(opts) => opts,
    };

    let (source, md) = match opts.path.as_deref() {
        Some("-") => (Source::Stdin, read_stdin().map_err(read_failure("stdin"))?),
        Some(p) => {
            let md = read_markdown_file(Path::new(p)).map_err(read_failure(p))?;
            (Source::File(PathBuf::from(p)), md)
        }
        None if !io::stdin().is_terminal() => {
            (Source::Stdin, read_stdin().map_err(read_failure("stdin"))?)
        }
        None => return Err(Failure::Usage("nenhum arquivo informado".to_string())),
    };

    let stdout_tty = io::stdout().is_terminal();
    if opts.plain || !stdout_tty {
        let w = opts.width.unwrap_or_else(|| {
            terminal::size()
                .map(|(c, _)| c as usize)
                .unwrap_or(80)
                .min(100)
        });
        let out = render::render(&md, w);
        let text = render::to_ansi(&out.lines, stdout_tty);
        return io::stdout()
            .write_all(text.as_bytes())
            .map_err(|e| Failure::Runtime(format!("não consegui escrever a saída: {e}")));
    }

    let mut app = App::new(source, md, opts.width.unwrap_or(100));
    let mut term = ratatui::init();
    let res = run(&mut term, &mut app);
    ratatui::restore();
    res.map_err(|e| Failure::Runtime(format!("erro no terminal: {e}")))
}

fn read_failure(what: &str) -> impl FnOnce(io::Error) -> Failure + '_ {
    move |e| Failure::Runtime(format!("não consegui ler {what}: {e}"))
}

/// Interpreta o valor de `-w`, limitando-o ao intervalo suportado pelo renderizador.
fn parse_width(value: &str) -> Result<usize, String> {
    value
        .parse()
        .map(render::clamp_width)
        .map_err(|_| format!("largura inválida: {value} (esperado um número inteiro positivo)"))
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

fn file_mtime(p: &PathBuf) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn run(term: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        term.draw(|f| app.draw(f))?;
        if event::poll(Duration::from_millis(250))? {
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

struct App {
    source: Source,
    name: String,
    md: String,
    mtime: Option<SystemTime>,
    max_width: usize,
    rendered: render::Rendered,
    render_width: usize,
    dirty: bool,
    scroll: usize,
    view_h: usize,
    input: Option<String>,
    query: String,
    matches: Vec<usize>,
    match_idx: usize,
}

impl App {
    fn new(source: Source, md: String, max_width: usize) -> Self {
        let (name, mtime) = match &source {
            Source::File(p) => (
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string()),
                file_mtime(p),
            ),
            Source::Stdin => ("stdin".to_string(), None),
        };
        App {
            source,
            name,
            md,
            mtime,
            max_width: max_width.max(20),
            rendered: render::Rendered::default(),
            render_width: 0,
            dirty: true,
            scroll: 0,
            view_h: 0,
            input: None,
            query: String::new(),
            matches: Vec::new(),
            match_idx: 0,
        }
    }

    fn max_scroll(&self) -> usize {
        self.rendered.lines.len().saturating_sub(self.view_h)
    }

    fn scroll_by(&mut self, d: isize) {
        let max = self.max_scroll() as isize;
        self.scroll = (self.scroll as isize + d).clamp(0, max) as usize;
    }

    fn scroll_to(&mut self, n: usize) {
        self.scroll = n.min(self.max_scroll());
    }

    fn rerender(&mut self, w: usize) {
        self.rendered = render::render(&self.md, w);
        self.render_width = w;
        self.dirty = false;
        self.refresh_matches();
    }

    fn refresh_matches(&mut self) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        let q = self.query.to_lowercase();
        for (i, l) in self.rendered.lines.iter().enumerate() {
            if line_text(l).to_lowercase().contains(&q) {
                self.matches.push(i);
            }
        }
        if self.match_idx >= self.matches.len() {
            self.match_idx = 0;
        }
    }

    fn check_reload(&mut self) {
        let Source::File(p) = &self.source else { return };
        let m = file_mtime(p);
        if m.is_some() && m != self.mtime {
            if let Ok(s) = read_markdown_file(p) {
                self.md = s;
                self.mtime = m;
                self.dirty = true;
            }
        }
    }

    fn set_search(&mut self, q: String) {
        self.query = q;
        self.refresh_matches();
        self.match_idx = self
            .matches
            .iter()
            .position(|&m| m >= self.scroll)
            .unwrap_or(0);
        self.goto_match();
    }

    fn goto_match(&mut self) {
        if let Some(&l) = self.matches.get(self.match_idx) {
            self.scroll_to(l.saturating_sub(self.view_h / 3));
        }
    }

    fn step_match(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.match_idx = if forward { (self.match_idx + 1) % n } else { (self.match_idx + n - 1) % n };
        self.goto_match();
    }

    /// Retorna `true` quando o usuário pediu para sair.
    fn on_key(&mut self, k: KeyEvent) -> bool {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return true;
        }

        if let Some(mut buf) = self.input.take() {
            match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.set_search(buf),
                KeyCode::Backspace => {
                    buf.pop();
                    self.input = Some(buf);
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    self.input = Some(buf);
                }
                _ => self.input = Some(buf),
            }
            return false;
        }

        let page = self.view_h.saturating_sub(1).max(1) as isize;
        let half = (self.view_h / 2).max(1) as isize;
        match k.code {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                if self.query.is_empty() {
                    return true;
                }
                self.query.clear();
                self.matches.clear();
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Char('f') => self.scroll_by(page),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('d') => self.scroll_by(half),
            KeyCode::Char('u') => self.scroll_by(-half),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(0),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(usize::MAX),
            KeyCode::Char(']') => {
                if let Some(&h) = self.rendered.headings.iter().find(|&&h| h > self.scroll) {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('[') => {
                if let Some(&h) = self.rendered.headings.iter().rev().find(|&&h| h < self.scroll) {
                    self.scroll_to(h);
                }
            }
            KeyCode::Char('/') => self.input = Some(String::new()),
            KeyCode::Char('n') => self.step_match(true),
            KeyCode::Char('N') => self.step_match(false),
            KeyCode::Char('r') => {
                if let Source::File(p) = &self.source {
                    if let Ok(s) = read_markdown_file(p) {
                        self.md = s;
                        self.mtime = file_mtime(p);
                        self.dirty = true;
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let [content, status] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

        let w = (content.width as usize)
            .saturating_sub(4)
            .min(self.max_width)
            .max(10);
        if self.dirty || w != self.render_width {
            self.rerender(w);
        }
        self.view_h = content.height as usize;
        self.scroll = self.scroll.min(self.max_scroll());

        let pad = (content.width as usize).saturating_sub(w) / 2;
        let inner = Rect {
            x: content.x + pad as u16,
            y: content.y,
            width: (w as u16).min(content.width),
            height: content.height,
        };
        let end = (self.scroll + self.view_h).min(self.rendered.lines.len());
        let q = self.query.to_lowercase();
        let visible: Vec<Line<'static>> = self.rendered.lines[self.scroll..end]
            .iter()
            .map(|l| if q.is_empty() { l.clone() } else { highlight_line(l, &q) })
            .collect();
        f.render_widget(Paragraph::new(visible), inner);

        let bar = Style::default()
            .bg(Color::Rgb(59, 66, 82))
            .fg(Color::Rgb(229, 233, 240));
        f.render_widget(
            Paragraph::new(self.status_line(status.width as usize)).style(bar),
            status,
        );
    }

    fn status_line(&self, width: usize) -> Line<'static> {
        let left = if let Some(buf) = &self.input {
            format!(" /{buf}█")
        } else {
            let max = self.max_scroll();
            let pct = if max == 0 { 100 } else { self.scroll * 100 / max };
            let mut s = format!(" {}  {pct}%", self.name);
            if !self.query.is_empty() {
                if self.matches.is_empty() {
                    s.push_str(&format!("  /{}: nada encontrado", self.query));
                } else {
                    s.push_str(&format!(
                        "  /{} [{}/{}]",
                        self.query,
                        self.match_idx + 1,
                        self.matches.len()
                    ));
                }
            }
            s
        };
        let right = "j/k rolar  [ ] títulos  / buscar  q sair ";
        let (lw, rw) = (left.width(), right.width());
        let text = if lw + rw < width {
            format!("{left}{}{right}", " ".repeat(width - lw - rw))
        } else {
            left
        };
        Line::from(text)
    }
}

fn line_text(l: &Line<'_>) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Destaca as ocorrências da busca (`q` já em minúsculas) dentro de cada trecho da linha.
fn highlight_line(line: &Line<'static>, q: &str) -> Line<'static> {
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
    fn parse_width_limita_aos_extremos() {
        assert_eq!(parse_width("5"), Ok(render::MIN_WIDTH));
        assert_eq!(parse_width("80"), Ok(80));
        assert_eq!(parse_width("18446744073709551615"), Ok(render::MAX_WIDTH));
    }

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

    #[test]
    fn read_markdown_file_rejeita_dispositivo() {
        let err = read_markdown_file(Path::new("/dev/zero")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn parse_width_rejeita_nao_numero() {
        assert!(parse_width("abc").unwrap_err().contains("abc"));
        assert!(parse_width("-3").is_err());
        assert!(parse_width("").is_err());
    }

    fn args(list: &[&str]) -> Result<Command, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    fn run_opts(plain: bool, width: Option<usize>, path: Option<&str>) -> Result<Command, String> {
        Ok(Command::Run(Options { plain, width, path: path.map(String::from) }))
    }

    #[test]
    fn parse_args_aceita_opcoes_e_arquivo() {
        assert_eq!(args(&["-p", "-w", "60", "a.md"]), run_opts(true, Some(60), Some("a.md")));
        assert_eq!(args(&["--width", "5", "-"]), run_opts(false, Some(render::MIN_WIDTH), Some("-")));
        assert_eq!(args(&[]), run_opts(false, None, None));
    }

    #[test]
    fn parse_args_rejeita_opcao_desconhecida_e_largura_ruim() {
        assert!(args(&["--zzz"]).unwrap_err().contains("--zzz"));
        assert!(args(&["-w"]).is_err());
        assert!(args(&["-w", "x"]).is_err());
        assert!(args(&["a.md", "b.md"]).is_err());
    }

    #[test]
    fn parse_args_duplo_hifen_trata_o_resto_como_caminho() {
        assert_eq!(args(&["--", "-p"]), run_opts(false, None, Some("-p")));
        assert!(args(&["--", "a", "b"]).is_err());
    }

    #[test]
    fn parse_args_ajuda_e_versao() {
        assert_eq!(args(&["-h"]), Ok(Command::Help));
        assert_eq!(args(&["a.md", "-V"]), Ok(Command::Version));
    }
}
