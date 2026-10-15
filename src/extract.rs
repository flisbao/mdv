//! Extração de trechos do Markdown para scripts e prompts: texto cru, sem renderizar.

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

use crate::render::{parse_options, sanitize};

/// Bloco de código do fonte: linguagem (primeira palavra do aviso) e texto cru.
pub struct Block {
    pub lang: String,
    pub text: String,
}

/// Título do fonte: nível, texto sem formatação e byte onde a linha do título começa.
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub start: usize,
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

pub fn headings(md: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut cur: Option<Heading> = None;
    for (ev, range) in Parser::new_ext(md, parse_options()).into_offset_iter() {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                let start = md[..range.start].rfind('\n').map_or(0, |i| i + 1);
                cur = Some(Heading { level: level as u8, title: String::new(), start });
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = &mut cur {
                    h.title.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(mut h) = cur.take() {
                    h.title = h.title.trim().to_string();
                    out.push(h);
                }
            }
            _ => {}
        }
    }
    out
}

/// Índice do título pedido: igualdade sem caixa, senão substring única, senão erro.
pub fn find_section(heads: &[Heading], query: &str) -> Result<usize, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Err("o título da seção está vazio".to_string());
    }
    if let Some(i) = heads.iter().position(|h| h.title.to_lowercase() == q) {
        return Ok(i);
    }
    let parecidos: Vec<usize> = (0..heads.len()).filter(|&i| heads[i].title.to_lowercase().contains(&q)).collect();
    match parecidos.as_slice() {
        [] => Err(format!("nenhuma seção com o título \"{}\"", query.trim())),
        [i] => Ok(*i),
        varios => {
            let nomes: Vec<&str> = varios.iter().map(|&i| heads[i].title.as_str()).collect();
            Err(format!("o título \"{}\" é ambíguo; candidatos: {}", query.trim(), nomes.join(", ")))
        }
    }
}

/// Markdown cru da seção `idx`: do título até o próximo de nível igual ou menor.
pub fn section_text(md: &str, heads: &[Heading], idx: usize) -> String {
    let h = &heads[idx];
    let end = heads[idx + 1..].iter().find(|n| n.level <= h.level).map_or(md.len(), |n| n.start);
    let mut text = md[h.start..end].trim_end().to_string();
    text.push('\n');
    text
}

/// Texto pedido pelas flags, já saneado.
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
    fn headings_ignora_cerca_de_codigo_e_front_matter() {
        let md = "---\ntitulo: x\n---\n\n# Real\n\n```\n# falso\n```\n\n## Outro\n";
        let h = headings(md);
        let titulos: Vec<&str> = h.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titulos, vec!["Real", "Outro"]);
        assert_eq!((h[0].level, h[1].level), (1, 2));
        assert!(md[h[0].start..].starts_with("# Real"));
    }

    #[test]
    fn find_section_prefere_igualdade_sem_caixa() {
        let h = headings("# Instalar\n\n# Instalar tudo\n");
        assert_eq!(find_section(&h, "instalar").unwrap(), 0);
    }

    #[test]
    fn find_section_aceita_substring_unica() {
        let h = headings("# Guia de uso\n\n# Notas\n");
        assert_eq!(find_section(&h, "uso").unwrap(), 0);
    }

    #[test]
    fn find_section_ambigua_lista_os_candidatos() {
        let h = headings("# Passo um\n\n# Passo dois\n");
        let e = find_section(&h, "passo").unwrap_err();
        assert!(e.contains("Passo um") && e.contains("Passo dois"), "{e}");
    }

    #[test]
    fn find_section_inexistente_ou_vazio_e_erro() {
        let h = headings("# A\n");
        assert!(find_section(&h, "zzz").unwrap_err().contains("zzz"));
        assert!(find_section(&h, "   ").is_err());
        assert!(find_section(&[], "a").is_err());
    }

    #[test]
    fn section_text_inclui_subsecoes_e_para_no_proximo_igual_ou_menor() {
        let h = headings(DOC);
        let a = section_text(DOC, &h, find_section(&h, "A").unwrap());
        assert!(a.starts_with("# A\n"));
        assert!(a.contains("## B") && a.contains("fn b() {}"));
        assert!(!a.contains("# C") && !a.contains("sem linguagem"));
        let b = section_text(DOC, &h, find_section(&h, "B").unwrap());
        assert!(b.starts_with("## B\n") && !b.contains("fn a()"));
    }

    #[test]
    fn section_text_da_ultima_secao_vai_ate_o_fim() {
        let h = headings(DOC);
        let c = section_text(DOC, &h, find_section(&h, "C").unwrap());
        assert!(c.ends_with("sem linguagem\n```\n"), "{c:?}");
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
