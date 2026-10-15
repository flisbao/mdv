//! `--check-links`: valida offline links relativos, âncoras e cercas de código sem fechar.
//! Trabalha sobre o fonte com `pulldown-cmark`, sem renderizar e sem rede.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};

use crate::extract::headings;
use crate::render::{parse_options, sanitize};

/// Teto de problemas listados (o restante é só contado).
pub const MAX_PROBLEMS: usize = 200;
/// Teto de links examinados por documento.
const MAX_LINKS: usize = 20_000;
/// Quantos outros `.md` são lidos para conferir `arquivo.md#âncora`.
const MAX_ANCHOR_FILES: usize = 50;
/// Maior `.md` lido para conferir âncoras.
const MAX_ANCHOR_FILE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Problem {
    /// Linha (1-based) do fonte.
    pub line: usize,
    pub msg: String,
}

/// Âncora de um título, como no GitHub: minúsculas, espaço vira `-`, pontuação some.
pub fn slugify(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// Âncoras do documento; títulos repetidos ganham `-1`, `-2`...
fn anchors_of(md: &str) -> HashSet<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = HashSet::new();
    for h in headings(md) {
        let base = slugify(&h.title);
        let n = seen.entry(base.clone()).or_insert(0);
        out.insert(if *n == 0 { base } else { format!("{base}-{n}") });
        *n += 1;
    }
    out
}

/// Decodifica `%XX` (bytes inválidos viram U+FFFD).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Esquema (`https:`, `mailto:`, `data:`...) ou `//host`: não é conferido (nada de rede).
fn is_external(dest: &str) -> bool {
    if dest.starts_with("//") {
        return true;
    }
    match dest.split_once(':') {
        Some((scheme, _)) => {
            !scheme.is_empty()
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
                && scheme.len() > 1 // `C:\x` (Windows) não é um esquema
        }
        None => false,
    }
}

/// Fechamento válido de uma cerca: última linha só com o caractere da abertura, ao menos tantas vezes.
fn fence_closed(block: &str) -> bool {
    let block = block.trim_end();
    let mut lines = block.lines();
    let Some(first) = lines.next() else { return false };
    let first = first.trim_start();
    let Some(ch) = first.chars().next().filter(|c| matches!(c, '`' | '~')) else { return true };
    let open = first.chars().take_while(|&c| c == ch).count();
    let Some(last) = lines.last() else { return false };
    let last = last.trim();
    last.chars().count() >= open && last.chars().all(|c| c == ch)
}

struct Checker<'a> {
    base: &'a Path,
    own: HashSet<String>,
    /// Âncoras dos outros `.md` já lidos (`None`: ilegível ou grande demais).
    files: HashMap<PathBuf, Option<HashSet<String>>>,
    problems: Vec<Problem>,
}

impl Checker<'_> {
    fn full(&self) -> bool {
        self.problems.len() >= MAX_PROBLEMS
    }

    fn add(&mut self, line: usize, msg: String) {
        if !self.full() {
            self.problems.push(Problem { line, msg });
        }
    }

    fn anchors_in(&mut self, path: &Path) -> Option<&HashSet<String>> {
        if !self.files.contains_key(path) {
            let loaded = if self.files.len() >= MAX_ANCHOR_FILES {
                None
            } else {
                std::fs::metadata(path)
                    .ok()
                    .filter(|m| m.is_file() && m.len() <= MAX_ANCHOR_FILE_BYTES)
                    .and_then(|_| std::fs::read_to_string(path).ok())
                    .map(|t| anchors_of(&t))
            };
            self.files.insert(path.to_path_buf(), loaded);
        }
        self.files[path].as_ref()
    }

    fn link(&mut self, line: usize, dest: &str) {
        let dest = dest.trim();
        if dest.is_empty() || is_external(dest) {
            return;
        }
        if let Some(frag) = dest.strip_prefix('#') {
            let frag = percent_decode(frag).to_lowercase();
            if !self.own.contains(&frag) {
                self.add(line, format!("âncora \"#{}\" não existe", sanitize(&frag)));
            }
            return;
        }
        let (path_part, frag) = match dest.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (dest, None),
        };
        let path_part = path_part.split('?').next().unwrap_or("");
        if path_part.is_empty() || path_part.starts_with('/') {
            return; // raiz do site ou do repositório: não dá para resolver aqui
        }
        let target = self.base.join(percent_decode(path_part));
        let Ok(meta) = std::fs::metadata(&target) else {
            self.add(line, format!("arquivo \"{}\" não encontrado", sanitize(path_part)));
            return;
        };
        let Some(frag) = frag.filter(|f| !f.is_empty()) else { return };
        let is_md = target
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
        if !meta.is_file() || !is_md {
            return;
        }
        let frag = percent_decode(frag).to_lowercase();
        let ok = self.anchors_in(&target).is_none_or(|set| set.contains(&frag));
        if !ok {
            self.add(
                line,
                format!("âncora \"#{}\" não existe em \"{}\"", sanitize(&frag), sanitize(path_part)),
            );
        }
    }
}

/// Problemas do documento, na ordem do fonte. `base` é a pasta que resolve os caminhos relativos.
pub fn check(md: &str, base: &Path) -> Vec<Problem> {
    let newlines: Vec<usize> = md.match_indices('\n').map(|(i, _)| i).collect();
    let line_of = |pos: usize| newlines.partition_point(|&n| n < pos) + 1;
    let mut ck = Checker { base, own: anchors_of(md), files: HashMap::new(), problems: Vec::new() };

    let broken: RefCell<Vec<(usize, String)>> = RefCell::new(Vec::new());
    let cb = |bl: pulldown_cmark::BrokenLink<'_>| {
        if matches!(bl.link_type, LinkType::Reference | LinkType::Collapsed) {
            broken.borrow_mut().push((bl.span.start, bl.reference.to_string()));
        }
        None
    };
    let parser = Parser::new_with_broken_link_callback(md, parse_options(), Some(cb));
    let mut links = 0;
    for (ev, range) in parser.into_offset_iter() {
        match ev {
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                links += 1;
                if links > MAX_LINKS {
                    break;
                }
                ck.link(line_of(range.start), &dest_url);
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) => {
                if !fence_closed(&md[range.clone()]) {
                    ck.add(line_of(range.start), "cerca de código aberta aqui e sem fechamento".to_string());
                }
            }
            Event::End(TagEnd::CodeBlock) => {}
            _ => {}
        }
        if ck.full() {
            break;
        }
    }
    for (pos, reference) in broken.into_inner() {
        ck.add(line_of(pos), format!("referência \"{}\" sem definição", sanitize(&reference)));
    }
    ck.problems.sort_by_key(|p| p.line);
    ck.problems
}

/// Relatório em texto puro: um problema por linha e o total.
pub fn format(label: &str, problems: &[Problem]) -> String {
    if problems.is_empty() {
        return "nenhum problema encontrado\n".to_string();
    }
    let mut out = String::new();
    for p in problems {
        let _ = writeln!(out, "{label}:{}: {}", p.line, p.msg);
    }
    let n = problems.len();
    let _ = writeln!(out, "{n} problema{}", if n == 1 { "" } else { "s" });
    if n >= MAX_PROBLEMS {
        let _ = writeln!(out, "(limite de {MAX_PROBLEMS} problemas listados; pode haver mais)");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(md: &str) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        check(md, dir.path()).into_iter().map(|p| format!("{}:{}", p.line, p.msg)).collect()
    }

    #[test]
    fn slug_segue_a_regra_do_github() {
        assert_eq!(slugify("Instalação Rápida"), "instalação-rápida");
        assert_eq!(slugify("O que é isso?"), "o-que-é-isso");
        assert_eq!(slugify("a_b - c"), "a_b---c");
        assert_eq!(slugify("  Olá, mundo!  "), "olá-mundo");
    }

    #[test]
    fn ancora_valida_nao_gera_problema_e_invalida_gera() {
        let md = "# Instalação\n\nVeja [aqui](#instalação) e [ali](#nao-existe).\n";
        assert_eq!(msgs(md), vec!["3:âncora \"#nao-existe\" não existe".to_string()]);
    }

    #[test]
    fn titulos_repetidos_ganham_sufixo() {
        let md = "# A\n\n# A\n\n[um](#a) [dois](#a-1) [tres](#a-2)\n";
        assert_eq!(msgs(md), vec!["5:âncora \"#a-2\" não existe".to_string()]);
    }

    #[test]
    fn arquivo_relativo_existente_e_ausente() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ok.md"), "# x\n").unwrap();
        let md = "[a](ok.md) [b](./falta.md) ![c](img/a.png)\n";
        let p = check(md, dir.path());
        let m: Vec<&str> = p.iter().map(|x| x.msg.as_str()).collect();
        assert_eq!(m, vec!["arquivo \"./falta.md\" não encontrado", "arquivo \"img/a.png\" não encontrado"]);
    }

    #[test]
    fn ancora_em_outro_arquivo_markdown() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.md"), "# Título B\n").unwrap();
        let md = "[ok](b.md#título-b) [ruim](b.md#outro)\n";
        let p = check(md, dir.path());
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].msg, "âncora \"#outro\" não existe em \"b.md\"");
    }

    #[test]
    fn links_externos_e_data_sao_ignorados() {
        let md = "[a](https://x.invalid/p) [b](mailto:a@b.c) [c](data:text/plain,oi) [d](//cdn.x/y.js)\n";
        assert!(msgs(md).is_empty());
    }

    #[test]
    fn link_dentro_de_codigo_e_ignorado() {
        let md = "```\n[a](falta.md)\n```\n\nTexto `[b](falta2.md)`.\n";
        assert!(msgs(md).is_empty());
    }

    #[test]
    fn cerca_de_codigo_sem_fechar() {
        assert_eq!(msgs("texto\n\n```rust\nfn a() {}\n"), vec!["3:cerca de código aberta aqui e sem fechamento".to_string()]);
        assert!(msgs("```rust\nfn a() {}\n```\n").is_empty());
        assert!(msgs("~~~\nx\n~~~\n").is_empty());
        assert_eq!(msgs("````\n```\nx\n```\n").len(), 1);
    }

    #[test]
    fn referencia_sem_definicao() {
        let md = "[ok][a] e [falta][b]\n\n[a]: https://x.invalid\n";
        assert_eq!(msgs(md), vec!["1:referência \"b\" sem definição".to_string()]);
    }

    #[test]
    fn colchetes_soltos_nao_sao_referencias() {
        assert!(msgs("Veja [1] e [x] no texto.\n").is_empty());
    }

    #[test]
    fn caminho_com_percentual_e_decodificado() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a b.md"), "x\n").unwrap();
        assert!(check("[a](a%20b.md)\n", dir.path()).is_empty());
    }

    #[test]
    fn percentual_perto_de_caractere_multibyte_nao_quebra() {
        let dir = tempfile::tempdir().unwrap();
        let p = check("[a](x%é.md) [b](100%)\n", dir.path());
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn texto_do_documento_na_mensagem_e_sanitizado() {
        let m = msgs("[a](falta\x1b[31m.md)\n");
        assert!(m.iter().all(|s| !s.contains('\x1b')), "{m:?}");
    }

    #[test]
    fn formato_lista_problemas_e_total() {
        let p = vec![
            Problem { line: 3, msg: "x".into() },
            Problem { line: 9, msg: "y".into() },
        ];
        assert_eq!(format("d.md", &p), "d.md:3: x\nd.md:9: y\n2 problemas\n");
        assert_eq!(format("d.md", &p[..1]), "d.md:3: x\n1 problema\n");
        assert_eq!(format("d.md", &[]), "nenhum problema encontrado\n");
    }

    #[test]
    fn teto_de_problemas() {
        let md: String = (0..MAX_PROBLEMS + 50).map(|i| format!("[a](#nao{i})\n")).collect();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(check(&md, dir.path()).len(), MAX_PROBLEMS);
    }
}
