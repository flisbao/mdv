//! Argumentos do `mdv-tools`: um subcomando e suas opções, parser manual (sem `clap`).

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    ExtractCode { lang: Option<String>, nth: Option<usize>, section: Option<String>, path: Option<String> },
    Section { title: String, path: Option<String> },
    CheckLinks { path: Option<String> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Run(Command),
    Help,
    Version,
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("a opção {flag} precisa de um valor"))
}

/// Um único caminho posicional opcional; o segundo é erro.
fn set_path(path: &mut Option<String>, arg: String) -> Result<(), String> {
    match path {
        Some(_) => Err(format!("argumento inesperado: {arg}")),
        None => {
            *path = Some(arg);
            Ok(())
        }
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut args = args.into_iter();
    let Some(sub) = args.next() else {
        return Err("informe um subcomando: extract-code, section ou check-links".to_string());
    };
    match sub.as_str() {
        "-h" | "--help" => Ok(Parsed::Help),
        "-V" | "--version" => Ok(Parsed::Version),
        "extract-code" => {
            let (mut lang, mut nth, mut section, mut path) = (None, None, None, None);
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--lang" => lang = Some(value(&mut args, &a)?),
                    "--section" => section = Some(value(&mut args, &a)?),
                    "-n" => {
                        let v = value(&mut args, &a)?;
                        nth = Some(match v.parse::<usize>() {
                            Ok(n) if n >= 1 => n,
                            _ => return Err(format!("-n precisa de um inteiro a partir de 1 (recebi \"{v}\")")),
                        });
                    }
                    "-" => set_path(&mut path, a)?,
                    f if f.starts_with('-') => return Err(format!("opção desconhecida: {f}")),
                    _ => set_path(&mut path, a)?,
                }
            }
            Ok(Parsed::Run(Command::ExtractCode { lang, nth, section, path }))
        }
        "section" => {
            let mut title = None;
            let mut path = None;
            for a in args {
                if a != "-" && a.starts_with('-') {
                    return Err(format!("opção desconhecida: {a}"));
                }
                if title.is_none() {
                    title = Some(a);
                } else {
                    set_path(&mut path, a)?;
                }
            }
            let title = title.ok_or("informe o título da seção")?;
            Ok(Parsed::Run(Command::Section { title, path }))
        }
        "check-links" => {
            let mut path = None;
            for a in args {
                if a != "-" && a.starts_with('-') {
                    return Err(format!("opção desconhecida: {a}"));
                }
                set_path(&mut path, a)?;
            }
            Ok(Parsed::Run(Command::CheckLinks { path }))
        }
        outro => Err(format!("subcomando desconhecido: {outro}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(a: &[&str]) -> Result<Parsed, String> {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn extract_code_com_opcoes() {
        let Parsed::Run(Command::ExtractCode { lang, nth, section, path }) =
            run(&["extract-code", "--lang", "rust", "-n", "2", "--section", "Uso", "a.md"]).unwrap()
        else {
            panic!("esperava ExtractCode")
        };
        assert_eq!((lang.as_deref(), nth, section.as_deref(), path.as_deref()), (Some("rust"), Some(2), Some("Uso"), Some("a.md")));
    }

    #[test]
    fn section_exige_titulo() {
        assert!(run(&["section"]).unwrap_err().contains("título"));
        let Parsed::Run(Command::Section { title, path }) = run(&["section", "Instalação", "-"]).unwrap() else {
            panic!()
        };
        assert_eq!((title.as_str(), path.as_deref()), ("Instalação", Some("-")));
    }

    #[test]
    fn check_links_aceita_so_um_caminho() {
        assert!(matches!(run(&["check-links", "a.md"]), Ok(Parsed::Run(Command::CheckLinks { .. }))));
        assert!(run(&["check-links", "a.md", "b.md"]).unwrap_err().contains("b.md"));
    }

    #[test]
    fn n_invalido_e_erro() {
        for v in ["0", "x", "-1", ""] {
            assert!(run(&["extract-code", "-n", v, "a.md"]).unwrap_err().contains("-n"), "{v}");
        }
        assert!(run(&["extract-code", "-n"]).is_err());
    }

    #[test]
    fn opcao_ou_subcomando_desconhecido() {
        assert!(run(&["extract-code", "--x"]).unwrap_err().contains("opção desconhecida: --x"));
        assert!(run(&["foo"]).unwrap_err().contains("subcomando desconhecido: foo"));
        assert!(run(&[]).unwrap_err().contains("subcomando"));
    }

    #[test]
    fn lang_e_n_so_valem_em_extract_code() {
        assert!(run(&["section", "A", "--lang", "rust"]).unwrap_err().contains("opção desconhecida: --lang"));
    }

    #[test]
    fn ajuda_e_versao() {
        assert!(matches!(run(&["--help"]), Ok(Parsed::Help)));
        assert!(matches!(run(&["-V"]), Ok(Parsed::Version)));
    }
}
