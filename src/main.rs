mod app;
mod browse;
mod cli;
mod clip;
mod config;
mod data;
mod doc;
mod graphics;
#[cfg(test)]
mod golden_ansi;
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

use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;

use app::{run_tui, App};
use load::{load_source, reads_stdin, Source, MAX_INPUT_BYTES};
use plain::{run_plain, write_stdout, HELP};

#[global_allocator]
static ALLOC: mem::CountingAlloc = mem::CountingAlloc;

/// Falha que encerra o programa; o código de saída depende do tipo.
pub(crate) enum Failure {
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
    let cli = cli::parse(std::env::args().skip(1)).map_err(Failure::Usage)?;
    match cli.command {
        cli::Command::Help => return write_stdout(HELP),
        cli::Command::Version => return write_stdout(&format!("mdv {}\n", env!("CARGO_PKG_VERSION"))),
        _ => {}
    }
    let limit = mem::resolve(cli.common.memory_buffer, std::env::var("MDV_MEMORY_BUFFER").ok().as_deref())
        .map_err(Failure::Usage)?;
    mem::set_limit(limit);
    mem::init();
    let mem_warning = mem::warn_if_above(limit, mem::available());
    let stdout_tty = io::stdout().is_terminal();
    let interactive = !cli.common.plain && stdout_tty;
    let width = cli.common.width.unwrap_or(render::MAX_WIDTH);
    if !interactive {
        if let Some(w) = &mem_warning {
            eprintln!("mdv: {w}");
        }
    }

    // `--follow` com stdin: a TUI abre na hora e o texto vai chegando aos poucos.
    if cli.common.follow && interactive && cli.command != cli::Command::Toc && reads_stdin(cli.common.path.as_deref()) {
        let rx = stream::spawn_stdin_reader(usize::try_from(MAX_INPUT_BYTES).unwrap_or(usize::MAX));
        let mut app = App::new(Source::Stdin, String::new(), width, &cli).with_stream(rx);
        app.notice = app.notice.or(mem_warning);
        return run_tui(app);
    }

    if interactive && cli.command == cli::Command::View(cli::Doc::Markdown) {
        if let Some(dir) = cli.common.path.as_deref().map(Path::new).filter(|p| p.is_dir()) {
            let picker = picker::Picker::scan(dir)
                .map_err(|e| Failure::Runtime(format!("não consegui ler {}: {e}", dir.display())))?;
            let mut app = App::new(Source::Stdin, String::new(), width, &cli);
            app.name = render::sanitize(&dir.display().to_string()).into_owned();
            app.modal = app::Modal::Picker(picker);
            app.notice = app.notice.or(mem_warning);
            return run_tui(app);
        }
    }
    let (source, md) = load_source(cli.common.path.as_deref())?;
    if cli.command == cli::Command::Stats {
        return write_stdout(&stats::format(&stats::stats(&md)));
    }
    if cli.command == cli::Command::Toc {
        return write_stdout(&render::toc(&render::render(&md, width)));
    }
    if interactive {
        let mut app = App::new(source, md, width, &cli);
        app.notice = app.notice.or(mem_warning);
        run_tui(app)
    } else {
        run_plain(&md, &source, cli.common.width, stdout_tty, &cli)
    }
}
