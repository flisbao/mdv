//! Extração de trechos do Markdown para scripts e prompts: texto cru, sem renderizar.

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

use mdv_text::{find_section, headings, parse_options, sanitize, section_text};

/// Bloco de código do fonte: linguagem (primeira palavra do aviso) e texto cru.
pub struct Block {
    pub lang: String,
    pub text: String,
}

pub struct ExtractOpts {
    /// Imprimir os blocos de código em vez do Markdown.
    pub code: bool,
    pub lang: Option<String>,
    pub nth: Option<usize>,
    pub section: Option<String>,
}

pub fn code_blocks(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    for ev in Parser::new_ext(md, parse_options()) {
        match ev {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                cur = Some(Block { lang, text: String::new() });
            }
            Event::Text(t) => {
                if let Some(b) = &mut cur {
                    b.text.push_str(&t);
                }
            }
            Event::End(TagEnd::CodeBlock) => out.extend(cur.take()),
            _ => {}
        }
    }
    out
}

pub fn extract(md: &str, o: &ExtractOpts) -> Result<String, String> {
    let scope: String;
    let text: &str = match &o.section {
        Some(q) => {
            let heads = headings(md);
            let i = find_section(&heads, q)?;
            scope = section_text(md, &heads, i);
            &scope
        }
        None => md,
    };
    if !o.code {
        return Ok(sanitize(text).into_owned());
    }
    let mut blocks = code_blocks(text);
    if let Some(l) = &o.lang {
        blocks.retain(|b| b.lang.eq_ignore_ascii_case(l));
    }
    if blocks.is_empty() {
        return Err(match &o.lang {
            Some(l) => format!("nenhum bloco de código em \"{l}\""),
            None => "nenhum bloco de código encontrado".to_string(),
        });
    }
    if let Some(n) = o.nth {
        if n == 0 || n > blocks.len() {
            return Err(format!("-n {n} fora do intervalo: há {} blocos", blocks.len()));
        }
        blocks = vec![blocks.swap_remove(n - 1)];
    }
    let partes: Vec<String> = blocks.iter().map(|b| sanitize(&b.text).into_owned()).collect();
    Ok(partes.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
intro

# A

texto A

```rust
fn a() {}
```

## B

```py
print('b')
```

```rust
fn b() {}
```

# C

```
sem linguagem
```
";

    fn opts(code: bool, lang: Option<&str>, nth: Option<usize>, section: Option<&str>) -> ExtractOpts {
        ExtractOpts {
            code,
            lang: lang.map(str::to_string),
            nth,
            section: section.map(str::to_string),
        }
    }

    #[test]
    fn code_blocks_le_linguagem_e_texto_cru() {
        let b = code_blocks(DOC);
        let langs: Vec<&str> = b.iter().map(|x| x.lang.as_str()).collect();
        assert_eq!(langs, vec!["rust", "py", "rust", ""]);
        assert_eq!(b[0].text, "fn a() {}\n");
        assert_eq!(b[3].text, "sem linguagem\n");
    }

    #[test]
    fn code_blocks_inclui_bloco_indentado_e_bloco_vazio() {
        let b = code_blocks("texto\n\n    recuado\n\n```\n```\n");
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].lang.as_str(), b[0].text.as_str()), ("", "recuado\n"));
        assert_eq!(b[1].text, "");
    }

    #[test]
    fn extract_codigo_separa_blocos_por_linha_em_branco() {
        let out = extract(DOC, &opts(true, None, None, None)).unwrap();
        assert_eq!(out, "fn a() {}\n\nprint('b')\n\nfn b() {}\n\nsem linguagem\n");
    }

    #[test]
    fn extract_filtra_por_linguagem_sem_caixa_e_n_vale_depois_do_filtro() {
        let todos = extract(DOC, &opts(true, Some("RUST"), None, None)).unwrap();
        assert_eq!(todos, "fn a() {}\n\nfn b() {}\n");
        let segundo = extract(DOC, &opts(true, Some("rust"), Some(2), None)).unwrap();
        assert_eq!(segundo, "fn b() {}\n");
    }

    #[test]
    fn extract_n_fora_do_intervalo_diz_quantos_existem() {
        let e = extract(DOC, &opts(true, Some("rust"), Some(3), None)).unwrap_err();
        assert!(e.contains("-n 3") && e.contains("2 blocos"), "{e}");
    }

    #[test]
    fn extract_sem_bloco_correspondente_e_erro_em_portugues() {
        let e = extract(DOC, &opts(true, Some("go"), None, None)).unwrap_err();
        assert!(e.contains("nenhum bloco de código") && e.contains("go"), "{e}");
        assert!(extract("só texto\n", &opts(true, None, None, None)).unwrap_err().contains("nenhum bloco"));
    }

    #[test]
    fn extract_secao_sozinha_imprime_o_markdown_cru() {
        let out = extract(DOC, &opts(false, None, None, Some("B"))).unwrap();
        assert!(out.starts_with("## B\n") && out.contains("print('b')") && out.ends_with("```\n"));
    }

    #[test]
    fn extract_secao_restringe_o_escopo_do_codigo() {
        let out = extract(DOC, &opts(true, None, None, Some("B"))).unwrap();
        assert_eq!(out, "print('b')\n\nfn b() {}\n");
        let so_c = extract(DOC, &opts(true, None, None, Some("C"))).unwrap();
        assert_eq!(so_c, "sem linguagem\n");
    }

    #[test]
    fn extract_sanitiza_controles() {
        let out = extract("```\na\x1b[31mb\n```\n", &opts(true, None, None, None)).unwrap();
        assert!(!out.contains('\x1b'));
        assert!(out.contains('\u{FFFD}'));
    }
}
