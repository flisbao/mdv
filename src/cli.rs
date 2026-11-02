//! Interpretação manual dos argumentos da linha de comando (sem dependência extra).
//!
//! `Cli { common, command }`: as opções comuns ficam em `Common`; as que só fazem sentido
//! para um formato existem apenas na variante de `Doc` correspondente, com uma função de
//! parsing por formato.

use std::path::Path;

use crate::{mem, render};

pub const DEFAULT_HEAD: usize = 1000;
pub const MAX_HEAD: usize = 100_000;

/// Como o JSON é mostrado. `Auto` vira tabela para lista de objetos (e JSONL) e texto nos demais casos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonView {
    Auto,
    Text,
    Table,
    Schema,
}

/// Protocolo gráfico para imagens (só no modo interativo).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Graphics {
    Off,
    Kitty,
    #[default]
    Auto,
}

/// Opções que valem para qualquer formato.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Common {
    pub path: Option<String>,
    pub plain: bool,
    pub width: Option<usize>,
    pub memory_buffer: Option<usize>,
    pub graphics: Graphics,
    pub remote: bool,
    pub follow: bool,
    /// `--table N`: abre o zoom da N-ésima tabela (vale para Markdown, JSON, YAML, XML e CSV/TSV).
    pub table: Option<usize>,
}

/// Formato do documento exibido, com as opções que só ele aceita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    Markdown,
    Json { json_path: Option<String>, view: JsonView },
    Yaml { yaml_path: Option<String>, view: JsonView },
    Xml { xml_path: Option<String>, view: JsonView },
    Table { delimiter: u8, head: usize, cols: Vec<String>, no_header: bool },
    /// Página HTML (`.html`, `.htm`, `.xhtml`): só o formato, sem opções próprias.
    Html,
}

impl Doc {
    /// Caminho e vista de um documento de dados (JSON, YAML ou XML); `None` nos demais formatos.
    pub fn data_view(&self) -> Option<(&Option<String>, JsonView)> {
        match self {
            Doc::Json { json_path: path, view }
            | Doc::Yaml { yaml_path: path, view }
            | Doc::Xml { xml_path: path, view } => Some((path, *view)),
            _ => None,
        }
    }

    pub fn data_view_mut(&mut self) -> Option<(&mut Option<String>, &mut JsonView)> {
        match self {
            Doc::Json { json_path: path, view }
            | Doc::Yaml { yaml_path: path, view }
            | Doc::Xml { xml_path: path, view } => Some((path, view)),
            _ => None,
        }
    }
}

/// O que fazer com a entrada. `Toc` e `Stats` valem só para Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    View(Doc),
    Toc,
    Stats,
    Help,
    Version,
    /// Converte para PDF, DOCX, ODT, XLSX ou ODS.
    Export(Export),
}

/// `-o arquivo`: converte a entrada e sai, sem abrir a TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub doc: Doc,
    pub output: String,
    pub force: bool,
    /// Só Markdown → planilha: tabelas da seção de título `section`.
    pub section: Option<String>,
}

/// Opções da linha de comando.
#[derive(Debug, Clone)]
pub struct Cli {
    pub common: Common,
    pub command: Command,
}

/// Uma opção já separada do resto: nome e, se a opção leva valor, o valor.
struct Flag {
    name: String,
    value: Option<String>,
}

impl Flag {
    fn value(&self) -> &str {
        self.value.as_deref().unwrap_or_default()
    }
}

const VALUE_FLAGS: &[&str] = &[
    "-w", "--width", "--path", "--table", "--memory-buffer", "--graphics", "--as", "--head", "--cols",
    "--delimiter", "-o", "--output", "--section",
];
const BOOL_FLAGS: &[&str] =
    &["-p", "--plain", "--remote", "--follow", "--toc", "--stats", "--no-header", "--force"];

/// Formato decidido antes de interpretar as opções específicas.
enum Format {
    Markdown,
    Json,
    Yaml,
    Xml,
    Html,
    Table(u8),
}

fn format_from_extension(path: &str) -> Format {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" | "jsonl" | "ndjson" => Format::Json,
        "yaml" | "yml" => Format::Yaml,
        "xml" => Format::Xml,
        "html" | "htm" | "xhtml" => Format::Html,
        "csv" => Format::Table(b','),
        "tsv" => Format::Table(b'\t'),
        _ => Format::Markdown,
    }
}

/// O `Doc` com as opções padrão do formato deduzido da extensão de `path`.
pub fn doc_for_path(path: &str) -> Doc {
    match format_from_extension(path) {
        Format::Markdown => Doc::Markdown,
        Format::Json => Doc::Json { json_path: None, view: JsonView::Auto },
        Format::Yaml => Doc::Yaml { yaml_path: None, view: JsonView::Auto },
        Format::Xml => Doc::Xml { xml_path: None, view: JsonView::Auto },
        Format::Html => Doc::Html,
        Format::Table(delimiter) => {
            Doc::Table { delimiter, head: DEFAULT_HEAD, cols: Vec::new(), no_header: false }
        }
    }
}

/// Interpreta o valor de `-w`, limitando-o ao intervalo suportado pelo renderizador.
fn parse_width(value: &str) -> Result<usize, String> {
    value
        .parse()
        .map(render::clamp_width)
        .map_err(|_| format!("largura inválida: {value} (esperado um número inteiro positivo)"))
}

fn parse_delimiter(v: &str) -> Result<u8, String> {
    match v {
        "\\t" | "tab" => Ok(b'\t'),
        s if s.len() == 1 && s.is_ascii() => Ok(s.as_bytes()[0]),
        _ => Err(format!("delimitador inválido: {v}")),
    }
}

/// Interpreta os argumentos (sem o nome do programa). `--` encerra as opções.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut args = args.into_iter().peekable();
    let subcommand = match args.peek().map(String::as_str) {
        Some("json") => Some(Format::Json),
        Some("yaml") => Some(Format::Yaml),
        Some("xml") => Some(Format::Xml),
        Some("csv") => Some(Format::Table(b',')),
        Some("tsv") => Some(Format::Table(b'\t')),
        _ => None,
    };
    if subcommand.is_some() {
        args.next();
    }

    let mut common = Common::default();
    let mut flags = Vec::new();
    let mut only_paths = false;
    while let Some(a) = args.next() {
        if only_paths || a == "-" || !a.starts_with('-') {
            if common.path.replace(a).is_some() {
                return Err("informe um único arquivo".to_string());
            }
            continue;
        }
        match a.as_str() {
            "--" => only_paths = true,
            "-h" | "--help" => return Ok(Cli { common, command: Command::Help }),
            "-V" | "--version" => return Ok(Cli { common, command: Command::Version }),
            n if VALUE_FLAGS.contains(&n) => {
                let missing = if matches!(n, "-w" | "--width") { "uma largura" } else { "um valor" };
                let v = args.next().ok_or_else(|| format!("a opção {a} precisa de {missing}"))?;
                flags.push(Flag { name: a, value: Some(v) });
            }
            n if BOOL_FLAGS.contains(&n) => flags.push(Flag { name: a, value: None }),
            _ => return Err(format!("opção desconhecida: {a}")),
        }
    }

    let output = take_value(&mut flags, &["-o", "--output"]);
    let section = take_value(&mut flags, &["--section"]);
    let force = take_bool(&mut flags, "--force");
    if output.is_none() {
        if force {
            return Err("--force só vale com -o".to_string());
        }
        if section.is_some() {
            return Err("--section só vale com -o".to_string());
        }
    }
    if output.as_deref().is_some_and(|o| o.is_empty() || o.starts_with('-')) {
        return Err("a opção -o precisa de um nome de arquivo".to_string());
    }
    let had: Vec<String> = flags.iter().map(|f| f.name.clone()).collect();
    let md_out = output.as_deref().is_some_and(saida_markdown);

    // `--delimiter` força o modo tabela mesmo com outra extensão ou subcomando.
    let format = if flags.iter().any(|f| f.name == "--delimiter") {
        Format::Table(b',')
    } else if let Some(f) = subcommand {
        f
    } else {
        common.path.as_deref().map_or(Format::Markdown, format_from_extension)
    };
    let cli = match format {
        Format::Markdown => parse_markdown(common, &flags),
        Format::Json => parse_data(common, &flags, |p, v| Doc::Json { json_path: p, view: v }),
        Format::Yaml => parse_data(common, &flags, |p, v| Doc::Yaml { yaml_path: p, view: v }),
        Format::Xml => parse_data(common, &flags, |p, v| Doc::Xml { xml_path: p, view: v }),
        Format::Html => parse_html(common, &flags),
        Format::Table(delimiter) => parse_table(common, &flags, delimiter),
    }?;
    match output {
        Some(output) => into_export(cli, output, force, section, &had, md_out),
        None => Ok(cli),
    }
}

fn take_value(flags: &mut Vec<Flag>, names: &[&str]) -> Option<String> {
    let found = flags.iter().rev().find(|f| names.contains(&f.name.as_str())).map(|f| f.value().to_string());
    flags.retain(|f| !names.contains(&f.name.as_str()));
    found
}

fn take_bool(flags: &mut Vec<Flag>, name: &str) -> bool {
    let found = flags.iter().any(|f| f.name == name);
    flags.retain(|f| f.name != name);
    found
}

/// `-o` com extensão `.md`: o texto convertido de um `.msg`, `.docx` ou `.xlsx` gravado como Markdown.
pub(crate) fn saida_markdown(output: &str) -> bool {
    Path::new(output).extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// `-o`: troca a visualização por uma exportação, recusando o que não faz sentido com ela.
fn into_export(
    cli: Cli,
    output: String,
    force: bool,
    section: Option<String>,
    had: &[String],
    md_out: bool,
) -> Result<Cli, String> {
    let Cli { common, command } = cli;
    let doc = match command {
        Command::View(doc) => doc,
        Command::Toc => return Err("-o não combina com --toc".to_string()),
        Command::Stats => return Err("-o não combina com --stats".to_string()),
        outro => return Ok(Cli { common, command: outro }),
    };
    if common.follow {
        return Err("-o não combina com --follow".to_string());
    }
    let entrada_convertida = common.path.as_deref().is_some_and(|p| crate::load::e_convertido(Path::new(p)));
    if md_out && !(matches!(doc, Doc::Markdown) && entrada_convertida) {
        return Err(format!("-o .md só vale para {}", crate::load::extensoes_convertidas()));
    }
    match &doc {
        Doc::Json { .. } => return Err("JSON ainda não exporta (use .md, .csv ou .tsv)".to_string()),
        Doc::Yaml { .. } => return Err("YAML ainda não exporta (use .md, .csv ou .tsv)".to_string()),
        Doc::Xml { .. } => return Err("XML ainda não exporta (use .md, .csv ou .tsv)".to_string()),
        Doc::Html => return Err("HTML ainda não exporta (use .md, .csv ou .tsv como saída)".to_string()),
        Doc::Table { .. } => {
            if let Some(f) = ["--head", "--cols"].into_iter().find(|f| had.iter().any(|h| h == f)) {
                return Err(format!("a opção {f} não vale com -o (a exportação leva todas as linhas e colunas)"));
            }
            if section.is_some() {
                return Err("a opção --section só vale para Markdown".to_string());
            }
        }
        Doc::Markdown => {
            if section.is_some() && common.table.is_some() {
                return Err("--section não combina com --table".to_string());
            }
            if md_out && (section.is_some() || common.table.is_some()) {
                return Err("-o .md não combina com --section nem --table".to_string());
            }
        }
    }
    Ok(Cli { common, command: Command::Export(Export { doc, output, force, section }) })
}

/// Opções comuns: devolve `Ok(true)` se consumiu a flag.
fn take_common(common: &mut Common, f: &Flag) -> Result<bool, String> {
    let a = f.name.as_str();
    match a {
        "-p" | "--plain" => common.plain = true,
        "--remote" => common.remote = true,
        "--follow" => common.follow = true,
        "-w" | "--width" => common.width = Some(parse_width(f.value())?),
        "--table" => {
            let v = f.value();
            common.table = Some(match v.parse::<usize>() {
                Ok(n) if n >= 1 => n,
                _ => return Err(format!("--table inválido: {v} (esperado um número a partir de 1)")),
            });
        }
        "--memory-buffer" => {
            common.memory_buffer =
                Some(mem::parse_size(f.value()).map_err(|e| format!("--memory-buffer: {e}"))?);
        }
        "--graphics" => {
            common.graphics = match f.value() {
                "off" => Graphics::Off,
                "kitty" => Graphics::Kitty,
                "auto" => Graphics::Auto,
                v => return Err(format!("--graphics inválido: {v} (use off, kitty ou auto)")),
            };
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Flag não reconhecida pelo formato: "só vale para X" se existe em outro formato.
fn wrong_format(flag: &str) -> String {
    match flag {
        "--path" | "--as" => format!("a opção {flag} só vale para JSON, YAML e XML"),
        "--head" | "--cols" | "--no-header" | "--delimiter" => {
            format!("a opção {flag} só vale para CSV/TSV")
        }
        "--toc" | "--stats" => format!("{flag} só vale para Markdown"),
        _ => format!("opção desconhecida: {flag}"),
    }
}

fn parse_markdown(mut common: Common, flags: &[Flag]) -> Result<Cli, String> {
    let (mut toc, mut stats) = (false, false);
    for f in flags {
        if take_common(&mut common, f)? {
            continue;
        }
        match f.name.as_str() {
            "--toc" => toc = true,
            "--stats" => stats = true,
            n => return Err(wrong_format(n)),
        }
    }
    if stats {
        if toc {
            return Err("--stats não combina com --toc".to_string());
        }
        if common.follow {
            return Err("--stats não combina com --follow".to_string());
        }
    }
    let command = if stats {
        Command::Stats
    } else if toc {
        Command::Toc
    } else {
        Command::View(Doc::Markdown)
    };
    Ok(Cli { common, command })
}

/// JSON, YAML e XML aceitam as mesmas opções (`--path`, `--as`); `make` escolhe a variante de `Doc`.
fn parse_data(mut common: Common, flags: &[Flag], make: fn(Option<String>, JsonView) -> Doc) -> Result<Cli, String> {
    let (mut path, mut view) = (None, JsonView::Auto);
    for f in flags {
        if take_common(&mut common, f)? {
            continue;
        }
        match f.name.as_str() {
            "--path" => path = Some(f.value().to_string()),
            "--as" => {
                view = match f.value() {
                    "auto" => JsonView::Auto,
                    "text" => JsonView::Text,
                    "table" => JsonView::Table,
                    "schema" => JsonView::Schema,
                    v => return Err(format!("vista inválida: {v} (use auto, text, table ou schema)")),
                };
            }
            n => return Err(wrong_format(n)),
        }
    }
    Ok(Cli { common, command: Command::View(make(path, view)) })
}

fn parse_table(mut common: Common, flags: &[Flag], mut delimiter: u8) -> Result<Cli, String> {
    let (mut head, mut cols, mut no_header) = (DEFAULT_HEAD, Vec::new(), false);
    for f in flags {
        if take_common(&mut common, f)? {
            continue;
        }
        match f.name.as_str() {
            "--no-header" => no_header = true,
            "--delimiter" => delimiter = parse_delimiter(f.value())?,
            "--head" => {
                let v = f.value();
                let n: usize = v.parse().map_err(|_| format!("--head inválido: {v}"))?;
                head = n.clamp(1, MAX_HEAD);
            }
            "--cols" => {
                cols = f
                    .value()
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
            }
            n => return Err(wrong_format(n)),
        }
    }
    Ok(Cli { common, command: Command::View(Doc::Table { delimiter, head, cols, no_header }) })
}

/// HTML não tem opções próprias: só as comuns valem; as de outros formatos são erro de uso.
fn parse_html(mut common: Common, flags: &[Flag]) -> Result<Cli, String> {
    for f in flags {
        if take_common(&mut common, f)? {
            continue;
        }
        return Err(wrong_format(f.name.as_str()));
    }
    Ok(Cli { common, command: Command::View(Doc::Html) })
}

#[cfg(test)]
mod tests;
