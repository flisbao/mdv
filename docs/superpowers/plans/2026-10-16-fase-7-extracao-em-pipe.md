# F7.5 — Extração em pipe (`--extract-code`, `--section`): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deixar scripts e prompts pegarem só um trecho do Markdown: `mdv --extract-code [--lang L] [-n N] ARQ` imprime o texto cru dos blocos de código e `mdv --section TÍTULO ARQ` imprime o Markdown cru de uma seção; as duas se combinam.

**Architecture:** Um módulo novo `src/extract.rs` trabalha só sobre o fonte com `pulldown-cmark` (sem renderização): `code_blocks`, `headings`, `find_section`, `section_text` e a função de entrada `extract`. O `cli.rs` ganha as flags e a validação; o `main.rs` chama `extract::extract` logo depois de carregar a entrada e antes de `--toc`/TUI/plain. Erros de casamento saem em português no stderr com código 1; flags inválidas, código 2.

**Tech Stack:** Rust 2021, `pulldown-cmark` 0.13 (já usado). Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md` (seção "F7.5 Extração em pipe"). Independente da fundação (`line_src`) e dos demais itens da Fase 7.

## Global Constraints

- Mensagens de erro, ajuda e README em português, com acentuação correta.
- A saída é texto puro: sem cor, sem formatação, sempre passando por `render::sanitize` (caracteres de controle viram U+FFFD).
- Código de saída: sem correspondência → mensagem em stderr e **1**; flag inválida ou combinação inválida → **2** (convenção da F6.1, `Failure::Usage`).
- `--lang` compara sem distinguir caixa; `-n` é 1-based e vale **depois** do filtro por linguagem.
- Título de seção: igualdade sem caixa; se não houver, substring única; se ambígua, erro listando os candidatos. Dois títulos exatamente iguais: vale o primeiro (ver Rulings na execução).
- Seção = do título até o próximo título de nível igual ou menor, ou o fim do documento.
- Vários blocos saem separados por uma linha em branco.
- Sem dependência nova. Memória: a entrada continua limitada por `MAX_INPUT_BYTES` e por `mem::check_parse`.
- Regra do `CLAUDE.md`: nenhum teste aloca memória sem teto; rode `(ulimit -v 6000000; cargo test 2>&1 | tail -20)`.
- `cargo clippy --all-targets` sem avisos novos.

## Review Focus

- Título inexistente, vazio ou só espaços: erro claro, exit 1 (o vazio é erro, não "casa com tudo") (Task 1).
- Cerca de código dentro de uma seção que o título contém (`## B` dentro de um bloco de código não é título): títulos só contam fora de código (Task 1).
- `-n 0`, `-n abc`, `--lang` sem `--extract-code`, `--section` em `json`/`csv`: exit 2 (Task 2).
- Documento com front matter YAML: o `---` do front matter não vira título nem bloco (Task 1).
- Bloco de código vazio (` ``` ` seguido de ` ``` `): conta como bloco, sai vazio, sem pânico (Task 1).

---

## File Structure

- `src/extract.rs` (novo): lógica pura de extração, com testes unitários.
- `src/render.rs`: `pub(crate) fn parse_options() -> Options` (mesmas opções do renderizador) e `sanitize` como `pub(crate)` (se a F7.4 ainda não o fez).
- `src/cli.rs`: campos `extract_code`, `lang`, `nth`, `section`; flags `--extract-code`, `--lang`, `-n`, `--section`; validação.
- `src/main.rs`: `mod extract;`, chamada em `run_cli`, linhas novas no `HELP`.
- `tests/cli.rs`: testes do binário.
- `README.md`, `CLAUDE.md`, `PLANO.md`: docs.

## Pontos do código atual que o plano usa

- `render_with` monta `Options::ENABLE_TABLES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS | ENABLE_FOOTNOTES | ENABLE_GFM | ENABLE_YAML_STYLE_METADATA_BLOCKS` inline; a extração precisa do mesmo conjunto para enxergar os mesmos títulos e blocos.
- `cli::parse` valida combinações no fim (`if cli.json_view != JsonView::Auto && cli.mode != Mode::Json`) e devolve `Err(String)` → `Failure::Usage` (exit 2).
- `run_cli` carrega a entrada com `load_source`/`load_latest`, trata `--toc` e depois decide TUI/plain. `Failure::Runtime(String)` → exit 1. `write_stdout(&str)` imprime tolerando pipe fechado.
- `tests/cli.rs` tem o helper `mdv(args, stdin) -> Output`; `tempfile` já é dependência.

---

### Task 1: Módulo `extract`

**Files:**
- Create: `src/extract.rs`
- Modify: `src/render.rs` (`parse_options`, visibilidade de `sanitize`), `src/main.rs` (`mod extract;`)
- Test: `src/extract.rs`

**Interfaces:**
- Consumes: `render::sanitize`, `render::parse_options` (este passo).
- Produces (usadas pelas Tasks 2–3):
  - `pub struct Block { pub lang: String, pub text: String }`
  - `pub struct Heading { pub level: u8, pub title: String, pub start: usize }` (`start` = byte do início da linha do título)
  - `pub fn code_blocks(md: &str) -> Vec<Block>`
  - `pub fn headings(md: &str) -> Vec<Heading>`
  - `pub fn find_section(heads: &[Heading], query: &str) -> Result<usize, String>`
  - `pub fn section_text(md: &str, heads: &[Heading], idx: usize) -> String`
  - `pub struct ExtractOpts { pub code: bool, pub lang: Option<String>, pub nth: Option<usize>, pub section: Option<String> }`
  - `pub fn extract(md: &str, o: &ExtractOpts) -> Result<String, String>`

- [ ] **Step 1: Escrever os testes que falham**

Em `src/render.rs`, antes de mexer: nada. Crie `src/extract.rs` só com o módulo de testes:

```rust
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
```

Em `src/main.rs`, acrescente `mod extract;` (depois de `mod data;`).

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv extract:: 2>&1 | tail -10)`
Expected: erro de compilação (`code_blocks`, `ExtractOpts`... não definidos).

- [ ] **Step 3: Implementar**

Em `src/render.rs`: garanta `pub(crate) fn sanitize` (se ainda for `fn sanitize`), acrescente

```rust
/// Opções do `pulldown-cmark` usadas em todo o programa (renderização e extração veem a mesma estrutura).
pub(crate) fn parse_options() -> Options {
    // `ENABLE_MATH` fica de fora de propósito: textos de LLM com `$5 e $10` seriam lidos como fórmula.
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}
```

e, em `render_with`, troque a definição local de `parse_opts` (com seu comentário) por `let parse_opts = parse_options();`.

Acima do módulo `tests` de `src/extract.rs`:

```rust
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
```

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde. Ajustes prováveis: (a) se `extract_n_fora_do_intervalo...` reclamar de "2 blocos" com 1 bloco, o texto singular/plural é decidido por você, mas o teste fixa "2 blocos" para 2; (b) `extract_codigo_separa_blocos...`: o bloco vazio produz `""` e junta uma linha em branco a mais, e o teste do bloco vazio só chama `code_blocks`, então não afeta.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/extract.rs src/render.rs src/main.rs
git commit -m "feat(extract): blocos de código e seções do Markdown em texto cru (F7.5)"
```

---

### Task 2: Flags `--extract-code`, `--lang`, `-n`, `--section`

**Files:**
- Modify: `src/cli.rs`
- Test: `src/cli.rs` (módulo `tests`)

**Interfaces:**
- Consumes: nada.
- Produces: `Cli.extract_code: bool`, `Cli.lang: Option<String>`, `Cli.nth: Option<usize>`, `Cli.section: Option<String>`; `Cli::wants_extract(&self) -> bool` (`extract_code || section.is_some()`).

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/cli.rs`:

```rust
    #[test]
    fn extract_code_e_suas_opcoes() {
        let c = run(&["--extract-code", "--lang", "rust", "-n", "2", "a.md"]).unwrap();
        assert!(c.extract_code && c.wants_extract());
        assert_eq!(c.lang.as_deref(), Some("rust"));
        assert_eq!(c.nth, Some(2));
        assert_eq!(c.section, None);
    }

    #[test]
    fn section_sozinha_e_valida() {
        let c = run(&["--section", "Instalação", "a.md"]).unwrap();
        assert!(c.wants_extract() && !c.extract_code);
        assert_eq!(c.section.as_deref(), Some("Instalação"));
    }

    #[test]
    fn sem_flags_de_extracao_nada_muda() {
        let c = run(&["a.md"]).unwrap();
        assert!(!c.wants_extract());
    }

    #[test]
    fn n_invalido_e_erro_de_uso() {
        for v in ["0", "-1", "x", ""] {
            let e = run(&["--extract-code", "-n", v, "a.md"]).unwrap_err();
            assert!(e.contains("-n"), "{v:?}: {e}");
        }
        assert!(run(&["--extract-code", "-n"]).is_err());
    }

    #[test]
    fn lang_e_n_exigem_extract_code() {
        assert!(run(&["--lang", "rust", "a.md"]).unwrap_err().contains("--extract-code"));
        assert!(run(&["-n", "1", "--section", "A", "a.md"]).unwrap_err().contains("--extract-code"));
    }

    #[test]
    fn extracao_so_vale_para_markdown() {
        assert!(run(&["json", "--section", "A", "d.json"]).unwrap_err().contains("Markdown"));
        assert!(run(&["csv", "--extract-code", "d.csv"]).unwrap_err().contains("Markdown"));
        assert!(run(&["--extract-code", "d.json"]).unwrap_err().contains("Markdown"), "modo deduzido da extensão");
    }

    #[test]
    fn extracao_nao_combina_com_toc_follow_latest() {
        for f in ["--toc", "--follow", "--latest"] {
            let e = run(&["--extract-code", f, "a.md"]).unwrap_err();
            assert!(e.contains(f), "{f}: {e}");
        }
    }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv cli:: 2>&1 | tail -8)`
Expected: erro de compilação (`extract_code`, `wants_extract`... não existem).

- [ ] **Step 3: Implementar**

Em `src/cli.rs`:

1. Campos em `Cli` (depois de `memory_buffer`) e valores no literal de `parse`:

```rust
    pub extract_code: bool,
    pub lang: Option<String>,
    pub nth: Option<usize>,
    pub section: Option<String>,
```
```rust
        extract_code: false,
        lang: None,
        nth: None,
        section: None,
```

2. Método:

```rust
impl Cli {
    /// `--extract-code` ou `--section`: imprime um trecho e sai, sem renderizar.
    pub fn wants_extract(&self) -> bool {
        self.extract_code || self.section.is_some()
    }
}
```

3. Braços em `parse` (perto de `--toc`):

```rust
            "--extract-code" => cli.extract_code = true,
            "--lang" => cli.lang = Some(value(&mut args, &a)?),
            "-n" => {
                let v = value(&mut args, &a)?;
                cli.nth = Some(match v.parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => return Err(format!("-n inválido: {v} (esperado um número a partir de 1)")),
                });
            }
            "--section" => cli.section = Some(value(&mut args, &a)?),
```

4. Validação, logo depois do `if cli.json_view != JsonView::Auto ...`:

```rust
    if (cli.lang.is_some() || cli.nth.is_some()) && !cli.extract_code {
        return Err("as opções --lang e -n só valem com --extract-code".to_string());
    }
    if cli.wants_extract() {
        if cli.mode != Mode::Markdown {
            return Err("--extract-code e --section só valem para Markdown".to_string());
        }
        for (on, flag) in [(cli.toc, "--toc"), (cli.follow, "--follow"), (cli.latest, "--latest")] {
            if on {
                return Err(format!("--extract-code e --section não combinam com {flag}"));
            }
        }
    }
```

(O teste "modo deduzido da extensão" passa porque a validação roda depois de `mode_from_extension`.)

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde. `cli::parse` já tem `#[allow(clippy::too_many_lines)]`.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): --extract-code, --lang, -n e --section (F7.5)"
```

---

### Task 3: Ligar no `main` e testar o binário

**Files:**
- Modify: `src/main.rs`
- Test: `tests/cli.rs`

**Interfaces:**
- Consumes: `extract::extract`, `extract::ExtractOpts` (Task 1); `Cli::wants_extract` e campos (Task 2).
- Produces: nada novo.

- [ ] **Step 1: Escrever os testes que falham**

Em `tests/cli.rs`:

```rust
const DOC_EXTRACAO: &str = "intro\n\n# A\n\n```rust\nfn a() {}\n```\n\n## B\n\n```py\nprint('b')\n```\n\n```rust\nfn b() {}\n```\n\n# C\n\n```\nsem lang\n```\n";

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn extract_code_imprime_todos_os_blocos_sem_cor() {
    let out = mdv(&["--extract-code", "-"], DOC_EXTRACAO);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "fn a() {}\n\nprint('b')\n\nfn b() {}\n\nsem lang\n");
    assert!(!stdout(&out).contains('\x1b'));
}

#[test]
fn extract_code_filtra_por_linguagem_e_n() {
    let out = mdv(&["--extract-code", "--lang", "RUST", "-n", "2", "-"], DOC_EXTRACAO);
    assert_eq!(stdout(&out), "fn b() {}\n");
}

#[test]
fn extract_code_n_fora_do_intervalo_sai_com_1() {
    let out = mdv(&["--extract-code", "--lang", "rust", "-n", "9", "-"], DOC_EXTRACAO);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("fora do intervalo"));
    assert!(stdout(&out).is_empty());
}

#[test]
fn extract_code_sem_bloco_sai_com_1() {
    let out = mdv(&["--extract-code", "-"], "só texto\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("nenhum bloco de código"));
}

#[test]
fn section_imprime_o_markdown_cru_com_subsecoes() {
    let out = mdv(&["--section", "a", "-"], DOC_EXTRACAO);
    let s = stdout(&out);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(s.starts_with("# A\n") && s.contains("## B") && !s.contains("# C"), "{s}");
}

#[test]
fn section_aninhada_para_no_proximo_titulo_do_mesmo_nivel() {
    let out = mdv(&["--section", "B", "-"], DOC_EXTRACAO);
    assert!(stdout(&out).starts_with("## B\n") && !stdout(&out).contains("fn a()"));
}

#[test]
fn section_ambigua_lista_candidatos_e_sai_com_1() {
    let out = mdv(&["--section", "passo", "-"], "# Passo um\n\n# Passo dois\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Passo um") && stderr(&out).contains("Passo dois"));
}

#[test]
fn section_inexistente_sai_com_1() {
    let out = mdv(&["--section", "zzz", "-"], DOC_EXTRACAO);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("zzz"));
}

#[test]
fn section_com_extract_code_extrai_so_dentro_da_secao() {
    let out = mdv(&["--section", "B", "--extract-code", "-"], DOC_EXTRACAO);
    assert_eq!(stdout(&out), "print('b')\n\nfn b() {}\n");
}

#[test]
fn extract_code_de_bloco_indentado() {
    let out = mdv(&["--extract-code", "-"], "texto\n\n    recuado\n");
    assert_eq!(stdout(&out), "recuado\n");
}

#[test]
fn extract_code_nao_deixa_escape_do_documento_passar() {
    let out = mdv(&["--extract-code", "-"], "```\na\x1b]0;titulo\x07b\n```\n");
    assert!(out.status.success());
    assert!(!stdout(&out).contains('\x1b') && !stdout(&out).contains('\x07'));
}

#[test]
fn extract_de_arquivo_funciona_como_de_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.md");
    std::fs::write(&p, DOC_EXTRACAO).unwrap();
    let out = mdv(&["--extract-code", "--lang", "py", p.to_str().unwrap()], "");
    assert_eq!(stdout(&out), "print('b')\n");
}

#[test]
fn flags_de_extracao_invalidas_sao_erro_de_uso() {
    for args in [
        &["--lang", "rust", "-"][..],
        &["--extract-code", "-n", "0", "-"][..],
        &["json", "--section", "A", "-"][..],
        &["--extract-code", "--toc", "-"][..],
    ] {
        let out = mdv(args, "# A\n");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --test cli extract 2>&1 | tail -12)`
Expected: FAIL (o binário ignora as flags e abre o modo normal; a saída não bate).

- [ ] **Step 3: Implementar**

Em `src/main.rs`, em `run_cli`, logo depois de o `let (source, md) = match &latest_dir { ... };` e antes de `if cli.toc && ...`:

```rust
    if cli.wants_extract() {
        mem::check_parse(md.len()).map_err(|e| Failure::Runtime(e.to_string()))?;
        let opts = extract::ExtractOpts {
            code: cli.extract_code,
            lang: cli.lang.clone(),
            nth: cli.nth,
            section: cli.section.clone(),
        };
        let text = extract::extract(&md, &opts).map_err(Failure::Runtime)?;
        return write_stdout(&text);
    }
```

No `HELP`, nas OPÇÕES (depois de `--memory-buffer`):

```
        --extract-code imprime só o texto dos blocos de código (sem cor)
        --lang <L>     com --extract-code: só os blocos da linguagem L
    -n <N>             com --extract-code: só o N-ésimo bloco (a partir de 1)
        --section <T>  imprime só o Markdown da seção de título T
```

Se o `--help` tiver teste de alinhamento ou de tamanho, ajuste. Cuide para o `--follow` com stdin (que abre a TUI antes de carregar) não passar por aqui: a Task 2 já recusa `--follow` com extração.

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -10`

```bash
git add src/main.rs tests/cli.rs
git commit -m "feat(cli): --extract-code e --section imprimem só o trecho pedido (F7.5)"
```

---

### Task 4: Documentação e amostra

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `PLANO.md`, `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`

- [ ] **Step 1: README**

- Bloco de ajuda (`<!-- mdv:help:start -->`): copie as quatro linhas novas do `HELP`.
- Seção nova "Extrair trechos" depois de "Markdown de agentes e LLMs":

```markdown
## Extrair trechos

Para scripts e prompts, o `mdv` imprime só o que você pede, em texto puro (sem cor):

```bash
mdv --extract-code plano.md                      # todos os blocos de código
mdv --extract-code --lang bash -n 2 plano.md     # o 2º bloco bash
mdv --section "Instalação" plano.md              # o Markdown da seção
mdv --section "Instalação" --extract-code plano.md | sh
```

`--lang` não distingue maiúsculas; `-n` conta a partir de 1, depois do filtro. O título casa por igualdade (sem distinguir caixa) ou por trecho único; se for ambíguo, o `mdv` lista os candidatos e sai com código 1, o mesmo código de quando nada é encontrado.
```

- [ ] **Step 2: CLAUDE.md, PLANO.md, spec**

- `CLAUDE.md`, em "Arquitetura": "- `src/extract.rs`: `--extract-code`/`--section`; trabalha sobre o fonte com `pulldown-cmark` (`render::parse_options`), sem renderizar, e devolve texto saneado."
- `PLANO.md` (gitignored): marque `[x] F7.5`.
- Spec: na seção F7.5, acrescente "Dois títulos iguais: vale o primeiro. `-n` fora do intervalo informa quantos blocos existem."

- [ ] **Step 3: Verificação final e commit**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED") && cargo clippy --all-targets 2>&1 | tail -5`
Expected: verde, sem avisos.

```bash
git add README.md CLAUDE.md docs/superpowers
git commit -m "docs: extração em pipe (--extract-code, --section) (F7.5)"
```

---

## Self-Review

**Cobertura da spec:** `--extract-code [--lang L] [-n N]` (filtro sem caixa, `-n` depois do filtro, blocos separados por linha em branco) → Tasks 1 e 3. `--section` (igualdade sem caixa → substring única → erro com candidatos) → `find_section`. Combinação (`--section` restringe o escopo) → `extract`. Saída sem cor e saneada → `sanitize`. Sem correspondência: stderr + exit 1; flag inválida: exit 2 → Tasks 2–3. Testes pedidos (filtro por linguagem, `-n` fora do intervalo, título ambíguo, seção aninhada, bloco indentado, ESC no código) → Tasks 1 e 3.

**Placeholders:** nenhum.

**Tipos:** `ExtractOpts { code, lang, nth, section }` é a mesma em `extract.rs`, `main.rs` e nos testes; `Cli.extract_code/lang/nth/section` e `wants_extract()` coincidem entre Task 2 e Task 3; `find_section(&[Heading], &str) -> Result<usize, String>` e `section_text(&str, &[Heading], usize) -> String` coincidem entre Task 1 e os testes.

**Review Focus → testes:** título vazio (`find_section_inexistente_ou_vazio_e_erro`); `# falso` em cerca e front matter (`headings_ignora_cerca_de_codigo_e_front_matter`); `-n 0`/`abc`, `--lang` sem `--extract-code`, `--section` em `json` (`n_invalido_e_erro_de_uso`, `lang_e_n_exigem_extract_code`, `extracao_so_vale_para_markdown`, `flags_de_extracao_invalidas_sao_erro_de_uso`); bloco vazio (`code_blocks_inclui_bloco_indentado_e_bloco_vazio`).
