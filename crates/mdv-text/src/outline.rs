use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::parse::parse_options;

/// Título do fonte: nível, texto sem formatação e byte onde a linha do título começa.
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub start: usize,
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

/// Markdown cru da seção `idx`: do título até o próximo de nível igual ou menor.
pub fn section_text(md: &str, heads: &[Heading], idx: usize) -> String {
    let mut text = md[crate::section_range(md, heads, idx)].trim_end().to_string();
    text.push('\n');
    text
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
    fn section_text_inclui_subsecoes_e_para_no_proximo_igual_ou_menor() {
        let h = headings(DOC);
        let a = section_text(DOC, &h, 0);
        assert!(a.starts_with("# A\n"));
        assert!(a.contains("## B") && a.contains("fn b() {}"));
        assert!(!a.contains("# C") && !a.contains("sem linguagem"));
        let b = section_text(DOC, &h, 1);
        assert!(b.starts_with("## B\n") && !b.contains("fn a()"));
    }

    #[test]
    fn section_text_da_ultima_secao_vai_ate_o_fim() {
        let h = headings(DOC);
        let c = section_text(DOC, &h, 2);
        assert!(c.ends_with("sem linguagem\n```\n"), "{c:?}");
    }
}
