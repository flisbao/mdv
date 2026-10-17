//! Interpretação manual dos argumentos da linha de comando (sem dependência extra).
//!
//! `Cli { common, command }`: as opções comuns ficam em `Common`; as que só fazem sentido
//! para um formato existem apenas na variante de `Doc` correspondente, com uma função de
//! parsing por formato.

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
    /// `--table N`: abre o zoom da N-ésima tabela (vale para Markdown, JSON e CSV/TSV).
    pub table: Option<usize>,
}

/// Formato do documento exibido, com as opções que só ele aceita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    Markdown,
    Json { json_path: Option<String>, view: JsonView },
    Table { delimiter: u8, head: usize, cols: Vec<String>, no_header: bool },
}

/// O que fazer com a entrada. `Toc` e `Stats` valem só para Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    View(Doc),
    Toc,
    Stats,
    Help,
    Version,
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
    "--delimiter",
];
const BOOL_FLAGS: &[&str] = &["-p", "--plain", "--remote", "--follow", "--toc", "--stats", "--no-header"];

/// Formato decidido antes de interpretar as opções específicas.
enum Format {
    Markdown,
    Json,
    Table(u8),
}

fn format_from_extension(path: &str) -> Format {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" | "jsonl" | "ndjson" => Format::Json,
        "csv" => Format::Table(b','),
        "tsv" => Format::Table(b'\t'),
        _ => Format::Markdown,
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

    // `--delimiter` força o modo tabela mesmo com outra extensão ou subcomando.
    let format = if flags.iter().any(|f| f.name == "--delimiter") {
        Format::Table(b',')
    } else if let Some(f) = subcommand {
        f
    } else {
        common.path.as_deref().map_or(Format::Markdown, format_from_extension)
    };
    match format {
        Format::Markdown => parse_markdown(common, &flags),
        Format::Json => parse_json(common, &flags),
        Format::Table(delimiter) => parse_table(common, &flags, delimiter),
    }
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
        "--path" | "--as" => format!("a opção {flag} só vale para JSON"),
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

fn parse_json(mut common: Common, flags: &[Flag]) -> Result<Cli, String> {
    let (mut json_path, mut view) = (None, JsonView::Auto);
    for f in flags {
        if take_common(&mut common, f)? {
            continue;
        }
        match f.name.as_str() {
            "--path" => json_path = Some(f.value().to_string()),
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
    Ok(Cli { common, command: Command::View(Doc::Json { json_path, view }) })
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
            (&["a.csv", "--path", ".x"], "a opção --path só vale para JSON"),
            (&["a.md", "--path", ".x"], "a opção --path só vale para JSON"),
            (&["a.csv", "--as", "text"], "a opção --as só vale para JSON"),
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

    #[test]
    fn opcoes_comuns_valem_em_todos_os_formatos() {
        for arq in ["a.md", "a.json", "a.csv"] {
            let c = run(&["-p", "-w", "50", "--table", "2", "--follow", arq]).unwrap().common;
            assert!(c.plain && c.follow);
            assert_eq!((c.width, c.table), (Some(50), Some(2)));
        }
    }
}
