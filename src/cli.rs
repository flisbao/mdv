//! Interpretação manual dos argumentos da linha de comando (sem dependência extra).

use crate::{mem, render};

pub const DEFAULT_HEAD: usize = 1000;
pub const MAX_HEAD: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Markdown,
    Json,
    Table { delimiter: u8 },
}

/// Como o JSON é mostrado. `Auto` vira tabela para lista de objetos (e JSONL) e texto nos demais casos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonView {
    Auto,
    Text,
    Table,
    Schema,
}

/// Protocolo gráfico para imagens (só no modo interativo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Graphics {
    Off,
    Kitty,
    Auto,
}

/// Opções da linha de comando para exibir um documento.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    pub plain: bool,
    pub width: Option<usize>,
    pub remote: bool,
    pub follow: bool,
    pub toc: bool,
    pub stats: bool,
    pub mode: Mode,
    pub path: Option<String>,
    pub json_path: Option<String>,
    pub json_view: JsonView,
    pub table: Option<usize>,
    pub memory_buffer: Option<usize>,
    pub head: usize,
    pub cols: Vec<String>,
    pub no_header: bool,
    pub graphics: Graphics,
}

#[allow(clippy::large_enum_variant)] // um único valor por execução; boxar só espalharia `*` nos testes
pub enum Parsed {
    Run(Cli),
    Help,
    Version,
}

fn mode_from_extension(path: &str) -> Mode {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" | "jsonl" | "ndjson" => Mode::Json,
        "csv" => Mode::Table { delimiter: b',' },
        "tsv" => Mode::Table { delimiter: b'\t' },
        _ => Mode::Markdown,
    }
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("a opção {flag} precisa de um valor"))
}

/// Interpreta o valor de `-w`, limitando-o ao intervalo suportado pelo renderizador.
fn parse_width(value: &str) -> Result<usize, String> {
    value
        .parse()
        .map(render::clamp_width)
        .map_err(|_| format!("largura inválida: {value} (esperado um número inteiro positivo)"))
}

/// Interpreta os argumentos (sem o nome do programa). `--` encerra as opções.
#[allow(clippy::too_many_lines)]
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut args = args.into_iter().peekable();
    let mut cli = Cli {
        plain: false,
        width: None,
        remote: false,
        follow: false,
        toc: false,
        stats: false,
        mode: Mode::Markdown,
        path: None,
        json_path: None,
        json_view: JsonView::Auto,
        table: None,
        memory_buffer: None,
        head: DEFAULT_HEAD,
        cols: Vec::new(),
        no_header: false,
        graphics: Graphics::Off,
    };
    let mut explicit_mode = true;
    match args.peek().map(String::as_str) {
        Some("json") => cli.mode = Mode::Json,
        Some("csv") => cli.mode = Mode::Table { delimiter: b',' },
        Some("tsv") => cli.mode = Mode::Table { delimiter: b'\t' },
        _ => explicit_mode = false,
    }
    if explicit_mode {
        args.next();
    }

    let mut only_paths = false;
    while let Some(a) = args.next() {
        if only_paths || a == "-" || !a.starts_with('-') {
            if cli.path.replace(a).is_some() {
                return Err("informe um único arquivo".to_string());
            }
            continue;
        }
        match a.as_str() {
            "--" => only_paths = true,
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-p" | "--plain" => cli.plain = true,
            "--remote" => cli.remote = true,
            "--follow" => cli.follow = true,
            "--toc" => cli.toc = true,
            "--stats" => cli.stats = true,
            "--no-header" => cli.no_header = true,
            "-w" | "--width" => {
                let v = args
                    .next()
                    .ok_or_else(|| format!("a opção {a} precisa de uma largura"))?;
                cli.width = Some(parse_width(&v)?);
            }
            "--path" => cli.json_path = Some(value(&mut args, &a)?),
                        "--table" => {
                            let v = value(&mut args, &a)?;
                            cli.table = Some(match v.parse::<usize>() {
                                Ok(n) if n >= 1 => n,
                                _ => return Err(format!("--table inválido: {v} (esperado um número a partir de 1)")),
                            });
                        }
            "--memory-buffer" => {
                let v = value(&mut args, &a)?;
                cli.memory_buffer =
                    Some(mem::parse_size(&v).map_err(|e| format!("--memory-buffer: {e}"))?);
            }
            "--graphics" => {
                let v = value(&mut args, &a)?;
                cli.graphics = match v.as_str() {
                    "off" => Graphics::Off,
                    "kitty" => Graphics::Kitty,
                    "auto" => Graphics::Auto,
                    _ => return Err(format!("--graphics inválido: {v} (use off, kitty ou auto)")),
                };
            }
            "--as" => {
                let v = value(&mut args, &a)?;
                cli.json_view = match v.as_str() {
                    "auto" => JsonView::Auto,
                    "text" => JsonView::Text,
                    "table" => JsonView::Table,
                    "schema" => JsonView::Schema,
                    _ => return Err(format!("vista inválida: {v} (use auto, text, table ou schema)")),
                };
            }
            "--head" => {
                let v = value(&mut args, &a)?;
                let n: usize = v.parse().map_err(|_| format!("--head inválido: {v}"))?;
                cli.head = n.clamp(1, MAX_HEAD);
            }
            "--cols" => {
                cli.cols = value(&mut args, &a)?
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
            }
            "--delimiter" => {
                let v = value(&mut args, &a)?;
                let b = match v.as_str() {
                    "\\t" | "tab" => b'\t',
                    s if s.len() == 1 && s.is_ascii() => s.as_bytes()[0],
                    _ => return Err(format!("delimitador inválido: {v}")),
                };
                cli.mode = Mode::Table { delimiter: b };
                explicit_mode = true;
            }
            _ => return Err(format!("opção desconhecida: {a}")),
        }
    }
    if !explicit_mode {
        if let Some(p) = cli.path.as_deref() {
            cli.mode = mode_from_extension(p);
        }
    }
    if cli.json_view != JsonView::Auto && cli.mode != Mode::Json {
        return Err("a opção --as só vale para JSON".to_string());
    }
    if cli.stats {
        if cli.mode != Mode::Markdown {
            return Err("--stats só vale para Markdown".to_string());
        }
        let others = [
            (cli.toc, "--toc"),
            (cli.follow, "--follow"),
        ];
        if let Some((_, flag)) = others.iter().find(|(on, _)| *on) {
            return Err(format!("--stats não combina com {flag}"));
        }
    }
    Ok(Parsed::Run(cli))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Cli, String> {
        match parse(args.iter().map(|s| s.to_string()))? {
            Parsed::Run(c) => Ok(c),
            _ => Err("esperava Run".into()),
        }
    }

    #[test]
    fn graphics_tem_padrao_off_e_aceita_kitty_e_auto() {
        assert_eq!(run(&["a.md"]).unwrap().graphics, Graphics::Off);
        assert_eq!(run(&["--graphics", "kitty", "a.md"]).unwrap().graphics, Graphics::Kitty);
        assert_eq!(run(&["--graphics", "auto", "a.md"]).unwrap().graphics, Graphics::Auto);
        assert_eq!(run(&["--graphics", "off", "a.md"]).unwrap().graphics, Graphics::Off);
    }

    #[test]
    fn graphics_invalido_ou_sem_valor_e_erro() {
        assert!(run(&["--graphics", "sixel", "a.md"]).unwrap_err().contains("sixel"));
        assert!(run(&["--graphics"]).is_err());
    }

    #[test]
    fn as_define_a_vista_do_json() {
        assert_eq!(run(&["json", "d.json", "--as", "table"]).unwrap().json_view, JsonView::Table);
        assert_eq!(run(&["json", "d.json", "--as", "schema"]).unwrap().json_view, JsonView::Schema);
        assert_eq!(run(&["json", "d.json", "--as", "text"]).unwrap().json_view, JsonView::Text);
        assert_eq!(run(&["json", "d.json", "--as", "auto"]).unwrap().json_view, JsonView::Auto);
        assert_eq!(run(&["d.json"]).unwrap().json_view, JsonView::Auto);
    }

    #[test]
    fn as_por_extensao_tambem_vale() {
        assert_eq!(run(&["d.jsonl", "--as", "table"]).unwrap().json_view, JsonView::Table);
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
        assert_eq!(c.mode, Mode::Markdown);
        assert_eq!(c.path.as_deref(), Some("a.md"));
    }

    #[test]
    fn subcomando_json_com_caminho() {
        let c = run(&["json", "d.json", "--path", ".a.b[0]"]).unwrap();
        assert_eq!(c.mode, Mode::Json);
        assert_eq!(c.json_path.as_deref(), Some(".a.b[0]"));
    }

    #[test]
    fn extensao_define_modo() {
        assert_eq!(run(&["x.csv"]).unwrap().mode, Mode::Table { delimiter: b',' });
        assert_eq!(run(&["x.tsv"]).unwrap().mode, Mode::Table { delimiter: b'\t' });
        assert_eq!(run(&["x.jsonl"]).unwrap().mode, Mode::Json);
    }

    #[test]
    fn tsv_subcomando_le_stdin() {
        let c = run(&["tsv", "-"]).unwrap();
        assert_eq!(c.mode, Mode::Table { delimiter: b'\t' });
        assert_eq!(c.path.as_deref(), Some("-"));
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
        assert_eq!(run(&["-w", "99999", "a.md"]).unwrap().width, Some(1000));
        assert_eq!(run(&["-w", "1", "a.md"]).unwrap().width, Some(20));
        assert_eq!(run(&["-w", "18446744073709551615"]).unwrap().width, Some(1000));
        assert_eq!(run(&["--width", "80", "-"]).unwrap().width, Some(80));
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
        assert_eq!(run(&["--", "-x.md"]).unwrap().path.as_deref(), Some("-x.md"));
        assert!(!run(&["--", "-p"]).unwrap().plain);
    }

    #[test]
    fn sem_argumentos_usa_padroes() {
        let c = run(&[]).unwrap();
        assert!(!c.plain && c.width.is_none() && c.path.is_none());
    }

    #[test]
    fn colunas_separadas_por_virgula() {
        let c = run(&["csv", "a.csv", "--cols", "id,nome"]).unwrap();
        assert_eq!(c.cols, vec!["id", "nome"]);
    }

    #[test]
    fn delimitador_e_head_validados() {
        let c = run(&["csv", "a.csv", "--delimiter", ";", "--head", "5", "--no-header"]).unwrap();
        assert_eq!(c.mode, Mode::Table { delimiter: b';' });
        assert_eq!((c.head, c.no_header), (5, true));
        assert!(run(&["csv", "a.csv", "--delimiter", "ab"]).is_err());
        assert!(run(&["csv", "a.csv", "--head", "x"]).unwrap_err().contains("--head"));
    }

    #[test]
    fn ajuda_e_versao() {
        assert!(matches!(parse(["-h".to_string()]), Ok(Parsed::Help)));
        assert!(matches!(parse(["a.md".to_string(), "-V".to_string()]), Ok(Parsed::Version)));
    }

        #[test]
        fn memory_buffer_aceita_tamanhos() {
            assert_eq!(run(&["--memory-buffer", "256M", "a.md"]).unwrap().memory_buffer, Some(256 * 1024 * 1024));
            assert_eq!(run(&["--memory-buffer", "1g", "a.md"]).unwrap().memory_buffer, Some(1024 * 1024 * 1024));
            assert_eq!(run(&["a.md"]).unwrap().memory_buffer, None);
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
            assert_eq!(run(&["--table", "2", "a.md"]).unwrap().table, Some(2));
            assert_eq!(run(&["a.md"]).unwrap().table, None);
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
        let cli = run(&["--delimiter", ";", "a.md"]).unwrap();
        assert_eq!(cli.mode, Mode::Table { delimiter: b';' });
    }

    #[test]
    fn delimiter_tab_e_barra_t() {
        for v in ["tab", "\\t"] {
            assert_eq!(run(&["--delimiter", v, "a.txt"]).unwrap().mode, Mode::Table { delimiter: b'\t' });
        }
        assert_eq!(run(&["--delimiter", "ab", "a.txt"]).unwrap_err(), "delimitador inválido: ab");
    }

    #[test]
    fn head_e_limitado_ao_intervalo() {
        assert_eq!(run(&["a.csv", "--head", "0"]).unwrap().head, 1);
        assert_eq!(run(&["a.csv", "--head", "999999999"]).unwrap().head, MAX_HEAD);
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
}
