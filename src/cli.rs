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
    /// `--table N`: abre o zoom da N-ésima tabela (vale para Markdown, JSON, YAML e CSV/TSV).
    pub table: Option<usize>,
}

/// Formato do documento exibido, com as opções que só ele aceita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    Markdown,
    Json { json_path: Option<String>, view: JsonView },
    Yaml { yaml_path: Option<String>, view: JsonView },
    Table { delimiter: u8, head: usize, cols: Vec<String>, no_header: bool },
    /// Página HTML (`.html`, `.htm`, `.xhtml`): só o formato, sem opções próprias.
    Html,
}

impl Doc {
    /// Caminho e vista de um documento de dados (JSON ou YAML); `None` nos demais formatos.
    pub fn data_view(&self) -> Option<(&Option<String>, JsonView)> {
        match self {
            Doc::Json { json_path: path, view } | Doc::Yaml { yaml_path: path, view } => Some((path, *view)),
            _ => None,
        }
    }

    pub fn data_view_mut(&mut self) -> Option<(&mut Option<String>, &mut JsonView)> {
        match self {
            Doc::Json { json_path: path, view } | Doc::Yaml { yaml_path: path, view } => Some((path, view)),
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
    Html,
    Table(u8),
}

fn format_from_extension(path: &str) -> Format {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" | "jsonl" | "ndjson" => Format::Json,
        "yaml" | "yml" => Format::Yaml,
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
        Format::Json => parse_data(common, &flags, false),
        Format::Yaml => parse_data(common, &flags, true),
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

/// `-o` com extensão `.md`: o texto convertido de um `.msg` ou `.docx` gravado como Markdown.
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
    let entrada_convertida = common.path.as_deref().is_some_and(|p| {
        let p = Path::new(p);
        crate::msg::is_msg(p) || crate::docx::is_docx(p)
    });
    if md_out && !(matches!(doc, Doc::Markdown) && entrada_convertida) {
        return Err("-o .md só vale para .msg e .docx".to_string());
    }
    match &doc {
        Doc::Json { .. } => return Err("JSON ainda não exporta (use .md, .csv ou .tsv)".to_string()),
        Doc::Yaml { .. } => return Err("YAML ainda não exporta (use .md, .csv ou .tsv)".to_string()),
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
        "--path" | "--as" => format!("a opção {flag} só vale para JSON e YAML"),
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

/// JSON e YAML aceitam as mesmas opções (`--path`, `--as`); `yaml` escolhe a variante de `Doc`.
fn parse_data(mut common: Common, flags: &[Flag], yaml: bool) -> Result<Cli, String> {
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
    let doc = if yaml { Doc::Yaml { yaml_path: path, view } } else { Doc::Json { json_path: path, view } };
    Ok(Cli { common, command: Command::View(doc) })
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
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Cli, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    fn doc(args: &[&str]) -> Doc {
        match run(args).unwrap().command {
            Command::View(d) => d,
            outro => panic!("{outro:?}"),
        }
    }

    fn tabela(args: &[&str]) -> (u8, usize, Vec<String>, bool) {
        match doc(args) {
            Doc::Table { delimiter, head, cols, no_header } => (delimiter, head, cols, no_header),
            outro => panic!("{outro:?}"),
        }
    }

    fn json_view(args: &[&str]) -> JsonView {
        match doc(args) {
            Doc::Json { view, .. } => view,
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn graphics_tem_padrao_auto_e_aceita_kitty_e_off() {
        assert_eq!(run(&["a.md"]).unwrap().common.graphics, Graphics::Auto);
        assert_eq!(run(&["--graphics", "kitty", "a.md"]).unwrap().common.graphics, Graphics::Kitty);
        assert_eq!(run(&["--graphics", "auto", "a.md"]).unwrap().common.graphics, Graphics::Auto);
        assert_eq!(run(&["--graphics", "off", "a.md"]).unwrap().common.graphics, Graphics::Off);
    }

    #[test]
    fn graphics_invalido_ou_sem_valor_e_erro() {
        assert!(run(&["--graphics", "sixel", "a.md"]).unwrap_err().contains("sixel"));
        assert!(run(&["--graphics"]).is_err());
    }

    #[test]
    fn as_define_a_vista_do_json() {
        assert_eq!(json_view(&["json", "d.json", "--as", "table"]), JsonView::Table);
        assert_eq!(json_view(&["json", "d.json", "--as", "schema"]), JsonView::Schema);
        assert_eq!(json_view(&["json", "d.json", "--as", "text"]), JsonView::Text);
        assert_eq!(json_view(&["json", "d.json", "--as", "auto"]), JsonView::Auto);
        assert_eq!(json_view(&["d.json"]), JsonView::Auto);
    }

    #[test]
    fn as_por_extensao_tambem_vale() {
        assert_eq!(json_view(&["d.jsonl", "--as", "table"]), JsonView::Table);
    }

    #[test]
    fn as_invalido_ou_fora_do_json_e_erro() {
        assert!(run(&["json", "--as", "xml", "-"]).unwrap_err().contains("xml"));
        assert!(run(&["--as"]).is_err());
        assert!(run(&["a.md", "--as", "table"]).unwrap_err().contains("--as"));
        assert!(run(&["csv", "a.csv", "--as", "table"]).unwrap_err().contains("--as"));
    }

    #[test]
    fn markdown_por_padrao() {
        let c = run(&["a.md"]).unwrap();
        assert_eq!(doc(&["a.md"]), Doc::Markdown);
        assert_eq!(c.common.path.as_deref(), Some("a.md"));
    }

    #[test]
    fn subcomando_json_com_caminho() {
        let c = run(&["json", "d.json", "--path", ".a.b[0]"]).unwrap();
        assert_eq!(
            c.command,
            Command::View(Doc::Json { json_path: Some(".a.b[0]".into()), view: JsonView::Auto })
        );
    }

    #[test]
    fn extensao_define_modo() {
        assert_eq!(tabela(&["x.csv"]).0, b',');
        assert_eq!(tabela(&["x.tsv"]).0, b'\t');
        assert!(matches!(doc(&["x.jsonl"]), Doc::Json { .. }));
    }

    #[test]
    fn tsv_subcomando_le_stdin() {
        let c = run(&["tsv", "-"]).unwrap();
        assert!(matches!(c.command, Command::View(Doc::Table { delimiter: b'\t', .. })));
        assert_eq!(c.common.path.as_deref(), Some("-"));
    }

    #[test]
    fn largura_invalida_e_erro() {
        assert!(run(&["-w", "abc", "a.md"]).is_err());
        assert!(run(&["-w"]).is_err());
        assert!(run(&["-w", "-3"]).is_err());
        assert!(run(&["-w", ""]).is_err());
        assert!(run(&["-w", "abc"]).unwrap_err().contains("abc"));
    }

    #[test]
    fn largura_e_limitada() {
        assert_eq!(run(&["-w", "99999", "a.md"]).unwrap().common.width, Some(1000));
        assert_eq!(run(&["-w", "1", "a.md"]).unwrap().common.width, Some(20));
        assert_eq!(run(&["-w", "18446744073709551615"]).unwrap().common.width, Some(1000));
        assert_eq!(run(&["--width", "80", "-"]).unwrap().common.width, Some(80));
    }

    #[test]
    fn flag_desconhecida_e_erro() {
        assert!(run(&["--nada", "a.md"]).unwrap_err().contains("--nada"));
    }

    #[test]
    fn mais_de_um_arquivo_e_erro() {
        assert!(run(&["a.md", "b.md"]).is_err());
        assert!(run(&["--", "a", "b"]).is_err());
    }

    #[test]
    fn duplo_traco_aceita_caminho_com_hifen() {
        assert_eq!(run(&["--", "-x.md"]).unwrap().common.path.as_deref(), Some("-x.md"));
        assert!(!run(&["--", "-p"]).unwrap().common.plain);
    }

    #[test]
    fn sem_argumentos_usa_padroes() {
        let c = run(&[]).unwrap();
        assert!(!c.common.plain && c.common.width.is_none() && c.common.path.is_none());
    }

    #[test]
    fn colunas_separadas_por_virgula() {
        assert_eq!(tabela(&["csv", "a.csv", "--cols", "id,nome"]).2, vec!["id", "nome"]);
    }

    #[test]
    fn delimitador_e_head_validados() {
        let (d, head, _, no_header) = tabela(&["csv", "a.csv", "--delimiter", ";", "--head", "5", "--no-header"]);
        assert_eq!((d, head, no_header), (b';', 5, true));
        assert!(run(&["csv", "a.csv", "--delimiter", "ab"]).is_err());
        assert!(run(&["csv", "a.csv", "--head", "x"]).unwrap_err().contains("--head"));
    }

    #[test]
    fn ajuda_e_versao() {
        assert!(matches!(parse(["-h".to_string()]), Ok(Cli { command: Command::Help, .. })));
        assert!(matches!(parse(["a.md".to_string(), "-V".to_string()]), Ok(Cli { command: Command::Version, .. })));
    }

        #[test]
        fn memory_buffer_aceita_tamanhos() {
            assert_eq!(run(&["--memory-buffer", "256M", "a.md"]).unwrap().common.memory_buffer, Some(256 * 1024 * 1024));
            assert_eq!(run(&["--memory-buffer", "1g", "a.md"]).unwrap().common.memory_buffer, Some(1024 * 1024 * 1024));
            assert_eq!(run(&["a.md"]).unwrap().common.memory_buffer, None);
        }

        #[test]
        fn memory_buffer_invalido_e_erro_de_uso() {
            for v in ["0", "-1", "1.5G", "abc", "", "16M", "999999999999999999999G"] {
                let e = run(&["--memory-buffer", v, "a.md"]).unwrap_err();
                assert!(e.contains("--memory-buffer"), "{v:?}: {e}");
            }
            assert!(run(&["--memory-buffer"]).is_err());
        }

        #[test]
        fn table_aceita_numero_a_partir_de_1() {
            assert_eq!(run(&["--table", "2", "a.md"]).unwrap().common.table, Some(2));
            assert_eq!(run(&["a.md"]).unwrap().common.table, None);
        }

        #[test]
        fn table_invalido_e_erro_de_uso() {
            for v in ["0", "-1", "x", ""] {
                let e = run(&["--table", v, "a.md"]).unwrap_err();
                assert!(e.contains("--table"), "{e}");
            }
            assert!(run(&["--table"]).is_err());
        }

    #[test]
    fn valor_faltando_no_fim_mantem_a_mensagem() {
        assert_eq!(run(&["a.md", "-w"]).unwrap_err(), "a opção -w precisa de uma largura");
        assert_eq!(run(&["a.md", "--path"]).unwrap_err(), "a opção --path precisa de um valor");
        assert_eq!(run(&["a.md", "--head"]).unwrap_err(), "a opção --head precisa de um valor");
        assert_eq!(run(&["a.md", "--cols"]).unwrap_err(), "a opção --cols precisa de um valor");
    }

    #[test]
    fn delimiter_forca_o_modo_tabela_mesmo_com_extensao_md() {
        assert_eq!(tabela(&["--delimiter", ";", "a.md"]).0, b';');
    }

    #[test]
    fn delimiter_tab_e_barra_t() {
        for v in ["tab", "\\t"] {
            assert_eq!(tabela(&["--delimiter", v, "a.txt"]).0, b'\t');
        }
        assert_eq!(run(&["--delimiter", "ab", "a.txt"]).unwrap_err(), "delimitador inválido: ab");
    }

    #[test]
    fn head_e_limitado_ao_intervalo() {
        assert_eq!(tabela(&["a.csv", "--head", "0"]).1, 1);
        assert_eq!(tabela(&["a.csv", "--head", "999999999"]).1, MAX_HEAD);
        assert_eq!(run(&["a.csv", "--head", "x"]).unwrap_err(), "--head inválido: x");
    }

    #[test]
    fn caminho_duplicado_e_graphics_invalido_mantem_mensagem() {
        assert_eq!(run(&["a.md", "b.md"]).unwrap_err(), "informe um único arquivo");
        assert_eq!(
            run(&["--graphics", "x", "a.md"]).unwrap_err(),
            "--graphics inválido: x (use off, kitty ou auto)"
        );
    }

    #[test]
    fn idle_foi_removido() {
        let e = run(&["--follow", "--idle", "5"]).unwrap_err();
        assert!(e.contains("opção desconhecida: --idle"), "{e}");
    }

    #[test]
    fn stats_nao_combina_com_toc_follow() {
        for f in ["--toc", "--follow"] {
            let e = run(&["--stats", f, "a.md"]).unwrap_err();
            assert!(e.contains("--stats") && e.contains(f), "{f}: {e}");
        }
    }

    #[test]
    fn stats_so_vale_para_markdown() {
        let e = run(&["csv", "--stats", "a.csv"]).unwrap_err();
        assert!(e.contains("--stats"), "{e}");
        assert!(run(&["--stats", "a.md"]).is_ok());
    }

    #[test]
    fn flags_de_outro_formato_sao_erro() {
        let casos: &[(&[&str], &str)] = &[
            (&["a.md", "--head", "5"], "a opção --head só vale para CSV/TSV"),
            (&["a.md", "--cols", "x"], "a opção --cols só vale para CSV/TSV"),
            (&["a.md", "--no-header"], "a opção --no-header só vale para CSV/TSV"),
            (&["a.json", "--head", "5"], "a opção --head só vale para CSV/TSV"),
            (&["a.csv", "--path", ".x"], "a opção --path só vale para JSON e YAML"),
            (&["a.md", "--path", ".x"], "a opção --path só vale para JSON e YAML"),
            (&["a.csv", "--as", "text"], "a opção --as só vale para JSON e YAML"),
            (&["a.json", "--toc"], "--toc só vale para Markdown"),
            (&["a.csv", "--toc"], "--toc só vale para Markdown"),
        ];
        for (args, msg) in casos {
            assert_eq!(run(args).unwrap_err(), *msg, "{args:?}");
        }
    }

    #[test]
    fn delimiter_em_markdown_vira_tabela_e_aceita_flags_de_tabela() {
        let cli = run(&["--delimiter", ";", "--head", "3", "a.txt"]).unwrap();
        match cli.command {
            Command::View(Doc::Table { delimiter, head, .. }) => assert_eq!((delimiter, head), (b';', 3)),
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn comandos_toc_stats_help_version() {
        assert!(matches!(run(&["--toc", "a.md"]).unwrap().command, Command::Toc));
        assert!(matches!(run(&["--stats", "a.md"]).unwrap().command, Command::Stats));
        assert!(matches!(run(&["-h"]).unwrap().command, Command::Help));
        assert!(matches!(run(&["a.md", "-V"]).unwrap().command, Command::Version));
    }

    fn export(args: &[&str]) -> Export {
        match run(args).unwrap().command {
            Command::Export(e) => e,
            outro => panic!("{outro:?}"),
        }
    }

    #[test]
    fn o_vira_export_com_o_formato_da_entrada() {
        let e = export(&["a.md", "-o", "a.pdf"]);
        assert_eq!((e.doc, e.output.as_str(), e.force, e.section), (Doc::Markdown, "a.pdf", false, None));
        let e = export(&["--output", "t.xlsx", "d.csv", "--force", "--no-header"]);
        assert_eq!(e.output, "t.xlsx");
        assert!(e.force);
        assert!(matches!(e.doc, Doc::Table { delimiter: b',', no_header: true, .. }));
        let e = export(&["tsv", "-", "-o", "t.ods"]);
        assert!(matches!(e.doc, Doc::Table { delimiter: b'\t', .. }));
    }

    #[test]
    fn saida_md_so_com_msg_e_docx() {
        let e = run(&["a.md", "-o", "b.md"]).unwrap_err();
        assert!(e.contains("só vale para .msg e .docx"), "{e}");
        let e = run(&["a.csv", "-o", "b.md"]).unwrap_err();
        assert!(e.contains("só vale para .msg e .docx"), "{e}");
        assert!(run(&["-", "-o", "b.md"]).unwrap_err().contains("só vale para .msg e .docx"));
        let c = run(&["a.MSG", "-o", "b.md"]).unwrap();
        assert!(matches!(c.command, Command::Export(Export { doc: Doc::Markdown, .. })));
        let c = run(&["a.docx", "-o", "b.md"]).unwrap();
        assert!(matches!(c.command, Command::Export(Export { doc: Doc::Markdown, .. })));
        assert!(run(&["a.msg", "-o", "b.md", "--section", "X"]).unwrap_err().contains("não combina"));
        assert!(run(&["a.msg", "-o", "b.md", "--table", "1"]).unwrap_err().contains("não combina"));
    }

    #[test]
    fn export_guarda_secao_e_table_fica_em_common() {
        let e = export(&["a.md", "-o", "t.xlsx", "--section", "Vendas"]);
        assert_eq!(e.section.as_deref(), Some("Vendas"));
        let c = run(&["a.md", "-o", "t.xlsx", "--table", "2"]).unwrap();
        assert_eq!(c.common.table, Some(2));
    }

    #[test]
    fn force_e_section_exigem_o() {
        assert!(run(&["a.md", "--force"]).unwrap_err().contains("--force só vale com -o"));
        assert!(run(&["a.md", "--section", "A"]).unwrap_err().contains("--section só vale com -o"));
    }

    #[test]
    fn o_nao_combina_com_toc_stats_follow() {
        for f in ["--toc", "--stats", "--follow"] {
            let e = run(&["a.md", "-o", "a.pdf", f]).unwrap_err();
            assert!(e.contains("-o") && e.contains(f), "{f}: {e}");
        }
    }

    #[test]
    fn section_so_para_markdown_e_nao_junto_de_table() {
        assert!(run(&["d.csv", "-o", "t.xlsx", "--section", "A"]).unwrap_err().contains("--section só vale para Markdown"));
        assert!(run(&["a.md", "-o", "t.xlsx", "--section", "A", "--table", "1"]).unwrap_err().contains("--section não combina com --table"));
    }

    #[test]
    fn head_e_cols_sao_recusados_com_o() {
        for f in [["--head", "5"], ["--cols", "a"]] {
            let e = run(&["d.csv", "-o", "t.xlsx", f[0], f[1]]).unwrap_err();
            assert!(e.contains(f[0]) && e.contains("-o"), "{e}");
        }
        assert!(run(&["d.csv", "--head", "5"]).is_ok(), "sem -o continua valendo");
    }

    #[test]
    fn yaml_deduzido_da_extensao() {
        for a in ["x.yaml", "x.YML"] {
            assert_eq!(doc(&[a]), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
        }
    }

    #[test]
    fn subcomando_yaml_com_caminho_e_vista() {
        assert_eq!(
            doc(&["yaml", "d.yml", "--path", ".a", "--as", "table"]),
            Doc::Yaml { yaml_path: Some(".a".into()), view: JsonView::Table }
        );
        assert_eq!(doc(&["yaml", "-"]), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
    }

    #[test]
    fn opcoes_de_csv_em_yaml_sao_erro() {
        assert_eq!(run(&["a.yaml", "--head", "5"]).unwrap_err(), "a opção --head só vale para CSV/TSV");
        assert_eq!(run(&["a.yaml", "--toc"]).unwrap_err(), "--toc só vale para Markdown");
    }

    #[test]
    fn yaml_nao_exporta() {
        assert!(run(&["a.yaml", "-o", "x.xlsx"]).unwrap_err().contains("YAML ainda não exporta"));
    }

    #[test]
    fn doc_for_path_devolve_o_padrao_de_cada_formato() {
        assert_eq!(doc_for_path("a.md"), Doc::Markdown);
        assert_eq!(doc_for_path("semextensao"), Doc::Markdown);
        assert_eq!(doc_for_path("a.json"), Doc::Json { json_path: None, view: JsonView::Auto });
        assert_eq!(doc_for_path("a.YAML"), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
        assert_eq!(doc_for_path("a.yml"), Doc::Yaml { yaml_path: None, view: JsonView::Auto });
        assert_eq!(
            doc_for_path("a.tsv"),
            Doc::Table { delimiter: b'\t', head: DEFAULT_HEAD, cols: vec![], no_header: false }
        );
    }

    #[test]
    fn data_view_expoe_caminho_e_vista_de_json_e_yaml() {
        let mut d = doc_for_path("a.yaml");
        let (path, view) = d.data_view_mut().unwrap();
        *path = Some(".x".into());
        *view = JsonView::Schema;
        assert_eq!(d.data_view(), Some((&Some(".x".to_string()), JsonView::Schema)));
        assert!(doc_for_path("a.json").data_view().is_some());
        assert!(doc_for_path("a.md").data_view().is_none());
        assert!(doc_for_path("a.csv").data_view_mut().is_none());
    }

    #[test]
    fn doc_for_path_html() {
        for a in ["a.html", "a.htm", "a.HTM", "a.xhtml"] {
            assert_eq!(doc_for_path(a), Doc::Html, "{a}");
        }
        assert_eq!(doc(&["página.HTML"]), Doc::Html);
    }

    #[test]
    fn html_nao_exporta() {
        let e = run(&["a.html", "-o", "x.pdf"]).unwrap_err();
        assert_eq!(e, "HTML ainda não exporta (use .md, .csv ou .tsv como saída)");
    }

    #[test]
    fn toc_e_stats_so_markdown_inclui_html() {
        assert_eq!(run(&["a.html", "--toc"]).unwrap_err(), "--toc só vale para Markdown");
        assert_eq!(run(&["a.htm", "--stats"]).unwrap_err(), "--stats só vale para Markdown");
    }

    #[test]
    fn head_em_html_e_erro() {
        assert_eq!(run(&["a.html", "--head", "5"]).unwrap_err(), "a opção --head só vale para CSV/TSV");
    }

    #[test]
    fn ajuda_cita_html() {
        assert!(crate::plain::HELP.contains("HTML"));
    }

    #[test]
    fn json_nao_exporta() {
        assert!(run(&["d.json", "-o", "t.xlsx"]).unwrap_err().contains("JSON"));
    }

    #[test]
    fn o_sem_valor_ou_vazio_e_erro() {
        assert!(run(&["a.md", "-o"]).is_err());
        assert!(run(&["a.md", "-o", ""]).unwrap_err().contains("-o"));
    }

    #[test]
    fn o_com_valor_que_parece_flag_e_erro() {
        for a in [&["a.md", "-o", "--force"][..], &["a.md", "-o", "-p"]] {
            let e = run(a).unwrap_err();
            assert!(e.contains("a opção -o precisa de um nome de arquivo"), "{e}");
        }
    }

    #[test]
    fn opcoes_comuns_valem_em_todos_os_formatos() {
        for arq in ["a.md", "a.json", "a.csv"] {
            let c = run(&["-p", "-w", "50", "--table", "2", "--follow", arq]).unwrap().common;
            assert!(c.plain && c.follow);
            assert_eq!((c.width, c.table), (Some(50), Some(2)));
        }
    }
}
