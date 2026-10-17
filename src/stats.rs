//! Estatísticas do Markdown (`--stats`): palavras, blocos de código e tokens estimados,
//! no documento e por seção. Trabalha sobre o fonte, sem renderizar.

use std::fmt::Write;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use mdv_text::{headings, parse_options, sanitize, section_text};

/// Teto de seções listadas; o que passar disso só entra no total.
const MAX_SECTIONS: usize = 10_000;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub words: usize,
    pub code_blocks: usize,
    /// Estimativa: caracteres do texto (prosa e código) divididos por 4, arredondado para cima.
    pub tokens: usize,
    pub tasks_done: usize,
    pub tasks_total: usize,
}

#[derive(Debug)]
pub struct Section {
    pub level: u8,
    pub title: String,
    pub counts: Counts,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub total: Counts,
    pub sections: Vec<Section>,
    /// Seções que passaram de `MAX_SECTIONS` e não foram listadas.
    pub omitted: usize,
}

fn counts(md: &str) -> Counts {
    let mut c = Counts::default();
    let mut chars = 0;
    let (mut in_code, mut in_meta) = (false, false);
    for ev in Parser::new_ext(md, parse_options()) {
        match ev {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code = true;
                c.code_blocks += 1;
            }
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::MetadataBlock(_)) => in_meta = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = false,
            Event::Text(t) | Event::Code(t) if !in_meta => {
                chars += t.chars().count();
                if !in_code {
                    c.words += t.split_whitespace().count();
                }
            }
            Event::TaskListMarker(done) => {
                c.tasks_total += 1;
                c.tasks_done += usize::from(done);
            }
            _ => {}
        }
    }
    c.tokens = chars.div_ceil(4);
    c
}

pub fn stats(md: &str) -> Stats {
    let heads = headings(md);
    let mut sections = Vec::new();
    for (i, h) in heads.iter().enumerate().take(MAX_SECTIONS) {
        sections.push(Section { level: h.level, title: h.title.clone(), counts: counts(&section_text(md, &heads, i)) });
    }
    Stats { total: counts(md), omitted: heads.len().saturating_sub(MAX_SECTIONS), sections }
}

/// Número com ponto como separador de milhar (`1.240`).
fn num(n: usize) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

fn line(c: &Counts) -> String {
    let words = if c.words == 1 { "palavra" } else { "palavras" };
    let blocks = if c.code_blocks == 1 { "bloco de código" } else { "blocos de código" };
    let mut out = format!("{} {words} · {} {blocks} · ~{} tokens", num(c.words), num(c.code_blocks), num(c.tokens));
    if c.tasks_total > 0 {
        let _ = write!(out, " · tarefas [{}/{}]", c.tasks_done, c.tasks_total);
    }
    out
}

pub fn format(s: &Stats) -> String {
    let mut out = format!("Total: {}\n", line(&s.total));
    for sec in &s.sections {
        let indent = "  ".repeat(usize::from(sec.level.saturating_sub(1)));
        let _ = writeln!(out, "{indent}{} {}", sanitize(&sec.title), line(&sec.counts));
    }
    if s.omitted > 0 {
        let _ = writeln!(out, "… {} seções não listadas", num(s.omitted));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documento_vazio_zera_tudo() {
        let s = stats("");
        assert_eq!(s.total, Counts::default());
        assert!(s.sections.is_empty());
    }

    #[test]
    fn palavras_contam_prosa_e_nao_codigo() {
        let c = counts("um dois `tres`\n\n```\nquatro cinco seis\n```\n");
        assert_eq!(c.words, 3);
        assert_eq!(c.code_blocks, 1);
    }

    #[test]
    fn front_matter_fica_de_fora() {
        let c = counts("---\ntitulo: muitas palavras aqui\n---\n\nso duas\n");
        assert_eq!(c.words, 2);
        assert_eq!(c.tokens, 2); // "so duas" = 7 caracteres, ceil(7/4)
    }

    #[test]
    fn tokens_somam_prosa_e_codigo_arredondando_para_cima() {
        // "abcde" (5) + "fghi\n" (5) = 10 caracteres -> ceil(10/4) = 3
        let c = counts("abcde\n\n```\nfghi\n```\n");
        assert_eq!(c.tokens, 3);
    }

    #[test]
    fn tarefas_contam_feitas_e_total() {
        let c = counts("- [x] a\n- [ ] b\n- [ ] c\n- item comum\n");
        assert_eq!((c.tasks_done, c.tasks_total), (1, 3));
    }

    #[test]
    fn secao_pai_soma_subsecoes_e_total_inclui_o_que_vem_antes_do_titulo() {
        let md = "antes do titulo\n\n# A\n\num dois\n\n## B\n\ntres\n\n```\nx\n```\n\n# C\n\nquatro\n";
        let s = stats(md);
        let w: Vec<(&str, usize)> = s.sections.iter().map(|x| (x.title.as_str(), x.counts.words)).collect();
        // palavras dos títulos também contam: A=1, B=1, C=1
        assert_eq!(w, vec![("A", 5), ("B", 2), ("C", 2)]);
        assert_eq!(s.sections[0].counts.code_blocks, 1);
        assert_eq!(s.sections[2].counts.code_blocks, 0);
        assert_eq!(s.total.words, 3 + 5 + 2);
    }

    #[test]
    fn formato_traz_total_e_uma_linha_por_secao_indentada() {
        let out = format(&stats("# A\n\num dois\n\n## B\n\n```\nx\n```\n"));
        let linhas: Vec<&str> = out.lines().collect();
        assert!(linhas[0].starts_with("Total: 4 palavras · 1 bloco de código · ~"), "{out}");
        assert!(linhas[1].starts_with("A ") && linhas[1].contains("4 palavras"), "{out}");
        assert!(linhas[2].starts_with("  B ") && linhas[2].contains("1 bloco de código"), "{out}");
    }

    #[test]
    fn formato_mostra_tarefas_so_quando_existem() {
        assert!(!format(&stats("# A\n\ntexto\n")).contains('['));
        assert!(format(&stats("# A\n\n- [x] a\n- [ ] b\n")).contains("[1/2]"));
    }

    #[test]
    fn formato_separa_milhares_com_ponto() {
        let md = format!("# A\n\n{}\n", "p ".repeat(1240));
        assert!(format(&stats(&md)).contains("1.241 palavras"));
    }

    #[test]
    fn formato_sanitiza_titulo() {
        let out = format(&stats("# a\x1b[31mb\n"));
        assert!(!out.contains('\x1b'));
    }
}
