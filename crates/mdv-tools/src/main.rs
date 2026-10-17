use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mdv_tools::args::{parse, Command, Parsed};
use mdv_tools::extract::{extract, ExtractOpts};
use mdv_tools::input::{read_limited, MAX_INPUT_BYTES};
use mdv_tools::links;
use mdv_text::sanitize;

const HELP: &str = "mdv-tools - ferramentas de linha de comando sobre Markdown

USO:
    mdv-tools extract-code [--lang <L>] [-n <N>] [--section <T>] [arquivo|-]
    mdv-tools section <T> [arquivo|-]
    mdv-tools check-links [arquivo|-]

SUBCOMANDOS:
    extract-code   imprime só o texto dos blocos de código (sem cor)
        --lang <L>     só os blocos da linguagem L
        -n <N>         só o N-ésimo bloco (a partir de 1)
        --section <T>  só os blocos dentro da seção de título T
    section        imprime só o Markdown da seção de título T
    check-links    confere offline links relativos, âncoras e cercas sem fechar; sai com 1 se houver problema

    -h, --help     mostra esta ajuda
    -V, --version  mostra a versão

Sem arquivo (ou com `-`), lê o documento de stdin.
";

enum Failure {
    Usage(String),
    Runtime(String),
    Findings,
}

enum Source {
    File(PathBuf),
    Stdin,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Findings) => ExitCode::from(1),
        Err(Failure::Runtime(m)) => {
            eprintln!("mdv-tools: {m}");
            ExitCode::from(1)
        }
        Err(Failure::Usage(m)) => {
            eprintln!("mdv-tools: {m}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), Failure> {
    match parse(std::env::args().skip(1)).map_err(Failure::Usage)? {
        Parsed::Help => write_stdout(HELP),
        Parsed::Version => write_stdout(&format!("mdv-tools {}\n", env!("CARGO_PKG_VERSION"))),
        Parsed::Run(Command::ExtractCode { lang, nth, section, path }) => {
            let (_, md) = load_source(path.as_deref())?;
            let opts = ExtractOpts { code: true, lang, nth, section };
            write_stdout(&extract(&md, &opts).map_err(Failure::Runtime)?)
        }
        Parsed::Run(Command::Section { title, path }) => {
            let (_, md) = load_source(path.as_deref())?;
            let opts = ExtractOpts { code: false, lang: None, nth: None, section: Some(title) };
            write_stdout(&extract(&md, &opts).map_err(Failure::Runtime)?)
        }
        Parsed::Run(Command::CheckLinks { path }) => {
            let (source, md) = load_source(path.as_deref())?;
            let (label, base) = match &source {
                Source::File(p) => (
                    sanitize(&p.display().to_string()).into_owned(),
                    p.parent()
                        .filter(|d| !d.as_os_str().is_empty())
                        .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
                ),
                Source::Stdin => ("<stdin>".to_string(), PathBuf::from(".")),
            };
            let problems = links::check(&md, &base);
            write_stdout(&links::format(&label, &problems))?;
            if problems.is_empty() { Ok(()) } else { Err(Failure::Findings) }
        }
    }
}

fn load_source(path: Option<&str>) -> Result<(Source, String), Failure> {
    let fail = |what: &str| {
        let what = what.to_string();
        move |e: io::Error| Failure::Runtime(sanitize(&format!("não consegui ler {what}: {e}")).into_owned())
    };
    match path {
        Some("-") => Ok((Source::Stdin, read_limited(io::stdin().lock(), MAX_INPUT_BYTES).map_err(fail("stdin"))?)),
        Some(p) => Ok((Source::File(PathBuf::from(p)), read_file(Path::new(p)).map_err(fail(p))?)),
        None if !io::stdin().is_terminal() => {
            Ok((Source::Stdin, read_limited(io::stdin().lock(), MAX_INPUT_BYTES).map_err(fail("stdin"))?))
        }
        None => Err(Failure::Usage("nenhum arquivo informado".to_string())),
    }
}

/// Lê um arquivo regular (nunca dispositivo, FIFO ou diretório) respeitando o teto.
fn read_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    if meta.len() > MAX_INPUT_BYTES {
        let msg = format!("entrada maior que {} MiB", MAX_INPUT_BYTES / (1024 * 1024));
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    read_limited(fs::File::open(path)?, MAX_INPUT_BYTES)
}

/// Escreve no stdout; pipe fechado (`| head -1`) não é erro.
fn write_stdout(s: &str) -> Result<(), Failure> {
    let mut out = io::stdout().lock();
    match out.write_all(s.as_bytes()).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        r => r.map_err(|e| Failure::Runtime(e.to_string())),
    }
}
