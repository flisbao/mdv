# F7.4 — Fundação (`line_src`, `code_blocks`, `tasks`) e copiar bloco/seção (`y`/`Y`): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dar ao `Rendered` o mapa linha renderizada → linha do fonte, o registro dos blocos de código e das tarefas (fundação usada por F7.1, F7.2, F7.3), e entregar as teclas `y` (copia o bloco de código) e `Y` (copia o Markdown cru da seção) via OSC 52.

**Architecture:** `render_with` passa a usar `Parser::into_offset_iter`; cada evento que abre um bloco de origem atualiza `Renderer.src_line`, e depois de cada evento `line_src` é preenchido até `lines.len()`. `render_code` registra um `CodeSpan`; o marcador de tarefa registra um `TaskItem`. Funções puras (`section_range`, `section_at`, `section_source`, `Rendered::code_block_at`) ficam em `render.rs`. Um módulo novo `src/clip.rs` traz o base64 e a sequência OSC 52; a TUI guarda a sequência em `App.pending_osc` e o `event_loop` a escreve no terminal.

**Tech Stack:** Rust 2021, `pulldown-cmark` 0.13 (já usado), `ratatui`. Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md` (seções "Fundação compartilhada" e "F7.4").

## Global Constraints

- Mensagens de erro, ajuda e README em português, com acentuação correta.
- A saída do `mdv` nunca contém escapes além do SGR gerado por ele; exceção deliberada: o OSC 52 de `y`/`Y`, emitido só pela TUI (nunca por `--plain`).
- Texto copiado ≤ 64 KiB (`MAX_COPY_BYTES = 64 * 1024`); maior: aviso "grande demais (N KiB)" e nada é enviado.
- O texto copiado passa por `sanitize` antes do base64.
- Sem dependência nova (base64 local, ~15 linhas).
- A fundação não muda nenhuma saída existente (`cargo test` verde sem alterar testes antigos).
- Regra do `CLAUDE.md`: nenhum teste aloca memória sem teto. Rode a suíte assim: `(ulimit -v 6000000; cargo test 2>&1 | tail -20)`.
- `cargo clippy --all-targets` sem avisos novos ao fim de cada tarefa.
- I/O fica fora de `render.rs`: a escrita do OSC 52 acontece em `main.rs`.

## Review Focus

- Documento sem título e `Y` pressionado: aviso "nenhuma seção aqui", nada copiado, sem pânico (Task 3).
- `Y` em seção que é a última do documento e em seção com subseções: o recorte vai até o próximo título de nível igual ou menor, ou até o fim do fonte (Task 1, `section_source`).
- Linha do fonte de uma lista aninhada e de uma citação: cada item aponta para a própria linha, não para a do pai (Task 1).
- Texto copiado com ESC/BEL no Markdown cru: vira U+FFFD antes do base64 (Task 2).
- `y` com o terminal no modo fonte (`m`) ou em JSON/CSV: não pode entrar em pânico; no modo fonte copia o texto do documento inteiro (limitado a 64 KiB) e em JSON/CSV avisa que não há bloco (Task 3).

---

## File Structure

- `src/render.rs`: novos tipos `CodeSpan`, `TaskItem`; campos novos de `Rendered`; `LineCursor`; funções `section_range`, `section_at`, `section_source`; `sanitize` passa a `pub(crate)`.
- `src/clip.rs` (novo): `base64`, `osc52`, `MAX_COPY_BYTES`.
- `src/main.rs`: `mod clip;`, `App.pending_osc`, helpers `ref_line`/`window_lines`/`scroll_to_line`, teclas `y`/`Y`, escrita do OSC no `event_loop`.
- `README.md`, `CLAUDE.md`: teclas e arquitetura.

## Pontos do código atual que o plano usa

- `Rendered` (`src/render.rs`, derive `Default`) tem `lines`, `headings`, `outline`, `tables`. `OutlineEntry { line, level, title }` guarda o índice (em `lines`) do título.
- `render_with` (linha ~230) itera `Parser::new_ext(md, parse_opts)`; depois de cada `r.handle(sanitize_event(ev))` checa `memory_error()`.
- `Renderer::finish` descarta linhas em branco finais. `render_code` empurra a borda superior, as linhas e a borda `╰─`; `Event::TaskListMarker(done)` chama `self.lists.set_task_marker(done)`.
- `App` (`src/main.rs`) usa `self.view.scroll` como linha de referência; `open_visible_table` usa `(scroll, scroll + height)`.
- Helper de teste existente: `test_app(Source::Stdin, md)` (módulo `tests` de `main.rs`) e, num submódulo, `ev(KeyCode)`/`tecla(&mut App, char)`; em `render.rs` o helper `render(md, width)`.

---

### Task 1: Fundação em `render.rs`

**Files:**
- Modify: `src/render.rs`
- Test: `src/render.rs` (módulo `tests`)

**Interfaces:**
- Consumes: nada.
- Produces (usadas pelas Tasks 2–3 e pelos planos F7.1, F7.2, F7.3):
  - `pub struct CodeSpan { pub start: usize, pub end: usize, pub lang: String, pub text: String }` (linhas renderizadas `[start, end)`, texto cru já saneado)
  - `pub struct TaskItem { pub line: usize, pub done: bool }`
  - `Rendered.line_src: Vec<usize>` (linha 0-based do fonte; mesmo comprimento de `lines` quando vem de `render_with`, vazio nos outros pontos de entrada), `Rendered.code_blocks: Vec<CodeSpan>`, `Rendered.tasks: Vec<TaskItem>`
  - `Rendered::code_block_at(&self, top: usize, bottom: usize) -> Option<&CodeSpan>`
  - `pub fn section_range(outline: &[OutlineEntry], idx: usize, total: usize) -> (usize, usize)`
  - `pub fn section_at(outline: &[OutlineEntry], line: usize) -> Option<usize>`
  - `pub fn section_source(md: &str, r: &Rendered, idx: usize) -> Option<String>`
  - `pub(crate) fn sanitize(s: &str) -> Cow<'_, str>` (hoje privada)

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/render.rs`, acrescente (o módulo já importa `super::*`):

```rust
    fn linha_de(r: &Rendered, i: usize) -> String {
        r.lines[i].spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn indice_da_linha_com(r: &Rendered, trecho: &str) -> usize {
        (0..r.lines.len())
            .find(|&i| linha_de(r, i).contains(trecho))
            .unwrap_or_else(|| panic!("nenhuma linha contém {trecho:?}"))
    }

    #[test]
    fn line_src_tem_uma_entrada_por_linha_renderizada() {
        let md = "# T\n\npar um\n\n- a\n- b\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> citação\n";
        let r = render(md, 40);
        assert_eq!(r.line_src.len(), r.lines.len());
    }

    #[test]
    fn line_src_aponta_para_a_linha_do_fonte() {
        let r = render("# Título\n\nprimeiro\n\nsegundo parágrafo\n", 40);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "Título")], 0);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "primeiro")], 2);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "segundo")], 4);
    }

    #[test]
    fn line_src_de_lista_aninhada_e_citacao_usa_a_linha_de_cada_item() {
        let r = render("- a\n  - b\n- c\n\n> q1\n> q2\n", 40);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "a")], 0);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "b")], 1);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "c")], 2);
        assert_eq!(r.line_src[indice_da_linha_com(&r, "q1")], 4);
    }

    #[test]
    fn linha_quebrada_pela_largura_herda_a_linha_do_bloco() {
        let md = format!("# T\n\n{}\n\nfim\n", "palavra ".repeat(30));
        let r = render(&md, 30);
        let do_bloco: Vec<usize> = (0..r.lines.len())
            .filter(|&i| linha_de(&r, i).contains("palavra"))
            .map(|i| r.line_src[i])
            .collect();
        assert!(do_bloco.len() > 1, "o parágrafo deveria quebrar em várias linhas");
        assert!(do_bloco.iter().all(|&s| s == 2), "{do_bloco:?}");
    }

    #[test]
    fn bloco_de_codigo_registra_intervalo_linguagem_e_texto_cru() {
        let r = render("x\n\n```rust\nfn a() {}\n```\n\n~~~\nsem lang\n~~~\n", 40);
        assert_eq!(r.code_blocks.len(), 2);
        let (a, b) = (&r.code_blocks[0], &r.code_blocks[1]);
        assert_eq!(a.lang, "rust");
        assert_eq!(a.text, "fn a() {}\n");
        assert!(linha_de(&r, a.start).starts_with("╭─"));
        assert!(linha_de(&r, a.end - 1).starts_with("╰─"));
        assert_eq!(b.lang, "");
        assert_eq!(b.text, "sem lang\n");
    }

    #[test]
    fn code_block_at_prefere_o_que_contem_a_linha_e_cai_para_o_visivel() {
        let r = render("```a\n1\n```\n\ntexto\n\n```b\n2\n```\n", 40);
        let (a, b) = (&r.code_blocks[0], &r.code_blocks[1]);
        assert_eq!(r.code_block_at(a.start, a.start + 5).map(|c| c.lang.as_str()), Some("a"));
        // Linha de referência no texto entre os blocos: o primeiro que cruza a janela.
        let texto = indice_da_linha_com(&r, "texto");
        assert_eq!(r.code_block_at(texto, b.end).map(|c| c.lang.as_str()), Some("b"));
        assert!(r.code_block_at(texto, texto + 1).is_none());
    }

    #[test]
    fn tarefas_registram_feita_ou_aberta_na_linha_do_item() {
        let r = render("# T\n\n- [x] a\n- [ ] b\n  - [x] c\n\n> - [ ] d\n", 40);
        let feitas: Vec<bool> = r.tasks.iter().map(|t| t.done).collect();
        assert_eq!(feitas, vec![true, false, true, false]);
        for t in &r.tasks {
            let marca = if t.done { '☑' } else { '☐' };
            assert!(linha_de(&r, t.line).contains(marca), "linha {}: {:?}", t.line, linha_de(&r, t.line));
        }
    }

    #[test]
    fn section_range_vai_ate_o_proximo_titulo_de_nivel_igual_ou_menor() {
        let r = render("# A\n\ntexto\n\n## B\n\nb\n\n## C\n\nc\n\n# D\n\nd\n", 40);
        let o = &r.outline;
        assert_eq!(section_range(o, 0, r.lines.len()), (o[0].line, o[3].line));
        assert_eq!(section_range(o, 1, r.lines.len()), (o[1].line, o[2].line));
        assert_eq!(section_range(o, 3, r.lines.len()), (o[3].line, r.lines.len()));
    }

    #[test]
    fn section_at_acha_o_titulo_mais_profundo_antes_da_linha() {
        let r = render("antes\n\n# A\n\ntexto\n\n## B\n\nb\n", 40);
        let o = &r.outline;
        assert_eq!(section_at(o, 0), None, "antes do primeiro título");
        assert_eq!(section_at(o, o[0].line), Some(0));
        assert_eq!(section_at(o, indice_da_linha_com(&r, "texto")), Some(0));
        assert_eq!(section_at(o, indice_da_linha_com(&r, "b")), Some(1));
    }

    #[test]
    fn section_source_devolve_o_markdown_cru_com_o_titulo() {
        let md = "intro\n\n# A\n\ntexto\n\n## B\n\nb\n\n# C\n\nc\n";
        let r = render(md, 40);
        assert_eq!(section_source(md, &r, 0).as_deref(), Some("# A\n\ntexto\n\n## B\n\nb\n"));
        assert_eq!(section_source(md, &r, 1).as_deref(), Some("## B\n\nb\n"));
        assert_eq!(section_source(md, &r, 2).as_deref(), Some("# C\n\nc\n"), "última seção vai ao fim");
        assert_eq!(section_source(md, &r, 9), None);
    }

    #[test]
    fn section_source_sem_mapa_de_linhas_devolve_none() {
        let r = error_rendered("x");
        assert_eq!(section_source("# A\n", &r, 0), None);
    }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv render::tests::line_src 2>&1 | tail -15)`
Expected: erro de compilação (`line_src`, `code_blocks`, `tasks`, `section_range`... não existem).

- [ ] **Step 3: Implementar**

Em `src/render.rs`:

1. `fn sanitize` → `pub(crate) fn sanitize` (só a visibilidade).

2. Tipos novos, junto de `OutlineEntry`:

```rust
/// Bloco de código desenhado: linhas renderizadas `[start, end)` e o texto cru (já saneado).
#[derive(Clone)]
pub struct CodeSpan {
    pub start: usize,
    pub end: usize,
    pub lang: String,
    pub text: String,
}

/// Item de lista de tarefas e a linha renderizada onde ele começa.
#[derive(Clone, Copy)]
pub struct TaskItem {
    pub line: usize,
    pub done: bool,
}
```

3. Campos novos em `Rendered` (depois de `tables`):

```rust
    /// Linha (0-based) do fonte onde cada linha renderizada começa. Vazio fora do Markdown.
    pub line_src: Vec<usize>,
    /// Blocos de código desenhados, em ordem.
    pub code_blocks: Vec<CodeSpan>,
    /// Itens de lista de tarefas, em ordem.
    pub tasks: Vec<TaskItem>,
```

4. Método (um `impl Rendered` novo, perto de `toc`):

```rust
impl Rendered {
    /// Bloco de código sob a linha `top`; se não houver, o primeiro que cruza a janela `[top, bottom)`.
    pub fn code_block_at(&self, top: usize, bottom: usize) -> Option<&CodeSpan> {
        self.code_blocks
            .iter()
            .find(|c| c.start <= top && top < c.end)
            .or_else(|| self.code_blocks.iter().find(|c| c.end > top && c.start < bottom))
    }
}
```

5. Funções de seção (perto de `toc`):

```rust
/// Linhas renderizadas `[início, fim)` da seção `idx`: do título até o próximo de nível
/// igual ou menor, ou até `total`.
pub fn section_range(outline: &[OutlineEntry], idx: usize, total: usize) -> (usize, usize) {
    let e = &outline[idx];
    let end = outline[idx + 1..].iter().find(|n| n.level <= e.level).map_or(total, |n| n.line);
    (e.line, end)
}

/// Título mais profundo que começa em `line` ou antes (a seção que contém a linha).
pub fn section_at(outline: &[OutlineEntry], line: usize) -> Option<usize> {
    outline.iter().rposition(|e| e.line <= line)
}

/// Markdown cru da seção `idx` (com o título), fatiado do fonte pelo mapa `line_src`.
pub fn section_source(md: &str, r: &Rendered, idx: usize) -> Option<String> {
    if idx >= r.outline.len() || r.line_src.len() != r.lines.len() {
        return None;
    }
    let (start, end) = section_range(&r.outline, idx, r.lines.len());
    let first = *r.line_src.get(start)?;
    let last = r.line_src.get(end).copied().unwrap_or_else(|| md.lines().count());
    let body: Vec<&str> = md.lines().skip(first).take(last.saturating_sub(first)).collect();
    let mut text = body.join("\n").trim_end().to_string();
    text.push('\n');
    Some(text)
}
```

6. Contador de linha do fonte (sem alocar um vetor de offsets: o documento pode ter 16 MiB de `\n`):

```rust
/// Converte offsets de byte (crescentes) em número de linha, contando `\n` de forma incremental.
#[derive(Default)]
struct LineCursor {
    pos: usize,
    line: usize,
}

impl LineCursor {
    fn line_at(&mut self, md: &str, off: usize) -> usize {
        if off < self.pos {
            *self = LineCursor::default();
        }
        let off = off.min(md.len());
        self.line += md.as_bytes()[self.pos..off].iter().filter(|&&b| b == b'\n').count();
        self.pos = off;
        self.line
    }
}

/// Início de um bloco que tem linha própria no fonte.
fn starts_source_block(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::List(_)
            | Tag::Item
            | Tag::Table(_)
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
    )
}
```

7. `Renderer` ganha `src_line: usize` (campo e `src_line: 0,` em `Renderer::new`). Em `finish`, depois do `while ... pop`, e antes de `self.out`:

```rust
        let n = self.out.lines.len();
        self.out.line_src.resize(n, self.src_line);
```

8. `render_with`: troque o laço por

```rust
    let mut r = Renderer::new(opts.width, opts.images);
    let mut cursor = LineCursor::default();
    for (ev, range) in Parser::new_ext(md, parse_opts).into_offset_iter() {
        // `Rule` e HTML geram linhas no próprio evento; um `Start` só vale para o que vem depois,
        // senão o texto do item pai (descarregado ao abrir uma sublista) herdaria a linha do filho.
        let before = matches!(ev, Event::Rule | Event::Html(_));
        let after = matches!(&ev, Event::Start(t) if starts_source_block(t));
        if before {
            r.src_line = cursor.line_at(md, range.start);
        }
        r.handle(sanitize_event(ev));
        r.out.line_src.resize(r.out.lines.len(), r.src_line);
        if after {
            r.src_line = cursor.line_at(md, range.start);
        }
        if let Some(err) = memory_error() {
            return err;
        }
    }
    r.finish()
```

9. Tarefa: em `handle`, troque `Event::TaskListMarker(done) => self.lists.set_task_marker(done),` por

```rust
            Event::TaskListMarker(done) => {
                self.out.tasks.push(TaskItem { line: self.out.lines.len(), done });
                self.lists.set_task_marker(done);
            }
```

10. Bloco de código: em `render_code`, logo antes de `let header = ...` acrescente `let start = self.out.lines.len();` e, no fim da função (depois do push de `╰─`):

```rust
        self.out.code_blocks.push(CodeSpan {
            start,
            end: self.out.lines.len(),
            lang: block.lang.clone(),
            text: block.buf.clone(),
        });
```

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: tudo verde, inclusive os testes antigos. Se `tarefas_registram_feita_ou_aberta_na_linha_do_item` falhar porque a linha do marcador cai numa linha em branco, a causa é uma linha empurrada entre o `Start(Item)` e o marcador: registre a linha como `lines.len()` **depois** de pular linhas em branco (procure o primeiro `lines[i]` não vazio a partir de `lines.len()` no momento do `End(Item)`), e anote um `Ruling:` no ledger. Se `section_source` falhar por uma linha em branco no recorte, confira se `outline[...].line` aponta para a linha do título (não para uma em branco anterior).

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/render.rs
git commit -m "feat(render): mapa linha→fonte, blocos de código e tarefas no Rendered (fundação da F7)"
```

---

### Task 2: `src/clip.rs` — base64 e OSC 52

**Files:**
- Create: `src/clip.rs`
- Modify: `src/main.rs` (`mod clip;`)
- Test: `src/clip.rs`

**Interfaces:**
- Consumes: `render::sanitize` (Task 1).
- Produces: `pub const MAX_COPY_BYTES: usize`, `pub fn base64(data: &[u8]) -> String`, `pub fn osc52(text: &str) -> Result<String, usize>` (`Err(n)` = tamanho em bytes do texto que passou do teto).

- [ ] **Step 1: Escrever os testes que falham**

Crie `src/clip.rs` só com:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_segue_os_vetores_da_rfc_4648() {
        let casos = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (entrada, saida) in casos {
            assert_eq!(base64(entrada.as_bytes()), saida, "entrada {entrada:?}");
        }
    }

    #[test]
    fn base64_aceita_bytes_arbitrarios() {
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64(&[0x00]), "AA==");
    }

    #[test]
    fn osc52_monta_a_sequencia_para_a_area_de_transferencia() {
        assert_eq!(osc52("abc").unwrap(), "\x1b]52;c;YWJj\x07");
    }

    #[test]
    fn osc52_sanitiza_controles_antes_do_base64() {
        assert_eq!(osc52("a\x1bb\x07c").unwrap(), osc52("a\u{FFFD}b\u{FFFD}c").unwrap());
        assert!(!osc52("a\x1b[31mb").unwrap().contains("[31m"), "o payload é só base64");
    }

    #[test]
    fn osc52_recusa_o_que_passa_do_teto_e_informa_o_tamanho() {
        let no_teto = "x".repeat(MAX_COPY_BYTES);
        assert!(osc52(&no_teto).is_ok());
        let grande = "x".repeat(MAX_COPY_BYTES + 1);
        assert_eq!(osc52(&grande), Err(MAX_COPY_BYTES + 1));
    }
}
```

Em `src/main.rs`, acrescente `mod clip;` (antes de `mod data;`, ordem alfabética).

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv clip:: 2>&1 | tail -10)`
Expected: erro de compilação (`base64`, `osc52`, `MAX_COPY_BYTES` não definidos).

- [ ] **Step 3: Implementar**

Acima do módulo `tests` de `src/clip.rs`:

```rust
//! Copiar para a área de transferência pelo terminal (OSC 52), sem `xclip`/`pbcopy`.

/// Maior texto enviado de uma vez; terminais costumam recusar payloads maiores.
pub const MAX_COPY_BYTES: usize = 64 * 1024;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 padrão (RFC 4648) com `=`.
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(chunk[0]) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        for i in 0..4 {
            if i <= chunk.len() {
                let idx = (n >> (18 - 6 * i)) & 63;
                out.push(char::from(ALPHABET[idx as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Sequência OSC 52 que copia `text`. `Err(n)` se o texto (em bytes) passa de `MAX_COPY_BYTES`.
pub fn osc52(text: &str) -> Result<String, usize> {
    let clean = crate::render::sanitize(text);
    if clean.len() > MAX_COPY_BYTES {
        return Err(clean.len());
    }
    Ok(format!("\x1b]52;c;{}\x07", base64(clean.as_bytes())))
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde. (Se o clippy reclamar do cast `idx as usize`, acrescente `#[allow(clippy::cast_possible_truncation)]` na função.)

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -10`

```bash
git add src/clip.rs src/main.rs
git commit -m "feat(clip): base64 e sequência OSC 52 (F7.4)"
```

---

### Task 3: Teclas `y` e `Y` na TUI

**Files:**
- Modify: `src/main.rs`
- Test: `src/main.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `Rendered::code_block_at`, `section_at`, `section_source` (Task 1); `clip::osc52` (Task 2).
- Produces (os planos F7.1, F7.2 e F7.3 dependem destes nomes):
  - `App::ref_line(&self) -> usize` — linha renderizada de referência (hoje: `view.scroll`).
  - `App::window_lines(&self) -> (usize, usize)` — `[topo, base)` de linhas renderizadas visíveis.
  - `App::scroll_to_line(&mut self, line: usize)` — rola para que a linha renderizada `line` fique no topo (hoje: `scroll_to`).
  - `App.pending_osc: Option<String>` — sequência a escrever no terminal.
  - `fn emit_osc(seq: &str)` — escreve e dá flush em `stdout`.

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/main.rs` (dentro do submódulo que já tem `ev(KeyCode)`, ao lado dos testes de zoom), acrescente:

```rust
        fn app_com_texto(md: &str) -> App {
            let mut app = test_app(Source::Stdin, md);
            app.rerender(60);
            app.view.height = 10;
            app
        }

        fn copiado(app: &App) -> Option<String> {
            app.pending_osc.clone()
        }

        #[test]
        fn y_copia_o_bloco_de_codigo_da_tela() {
            let mut app = app_com_texto("texto\n\n```rust\nfn a() {}\n```\n");
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), Some(clip::osc52("fn a() {}\n").unwrap()));
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("copiado: 1 linha")), "{:?}", app.notice);
        }

        #[test]
        fn y_sem_bloco_de_codigo_avisa_e_nao_copia() {
            let mut app = app_com_texto("só texto\n");
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhum bloco de código")));
        }

        #[test]
        fn y_em_json_nao_entra_em_panico_e_avisa() {
            let mut app = test_app(Source::Stdin, r#"[{"a":1}]"#);
            app.doc.mode = cli::Mode::Json;
            app.rerender(60);
            app.view.height = 10;
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.is_some());
        }

        #[test]
        fn y_no_modo_fonte_copia_o_documento() {
            let mut app = app_com_texto("# A\n\ntexto\n");
            app.on_key(ev(KeyCode::Char('m')));
            app.rerender(60);
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), Some(clip::osc52("# A\n\ntexto\n").unwrap()));
        }

        #[test]
        fn y_maiusculo_copia_o_markdown_cru_da_secao() {
            let mut app = app_com_texto("# A\n\ntexto\n\n## B\n\nb\n");
            app.on_key(ev(KeyCode::Char('Y')));
            assert_eq!(copiado(&app), Some(clip::osc52("# A\n\ntexto\n\n## B\n\nb\n").unwrap()));
        }

        #[test]
        fn y_maiusculo_usa_a_secao_da_primeira_linha_visivel() {
            let mut app = app_com_texto("# A\n\na\n\n# B\n\nb\n");
            let b = app.rendered.outline[1].line;
            app.view.scroll = b;
            app.on_key(ev(KeyCode::Char('Y')));
            assert_eq!(copiado(&app), Some(clip::osc52("# B\n\nb\n").unwrap()));
        }

        #[test]
        fn y_maiusculo_sem_titulo_avisa_e_nao_copia() {
            let mut app = app_com_texto("só texto\n");
            app.on_key(ev(KeyCode::Char('Y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma seção")));
        }

        #[test]
        fn copia_maior_que_64_kib_nao_envia_nada() {
            let md = format!("```\n{}\n```\n", "x".repeat(70 * 1024));
            let mut app = app_com_texto(&md);
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(copiado(&app), None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("grande demais (70 KiB)")), "{:?}", app.notice);
        }

        #[test]
        fn ref_line_e_window_lines_seguem_a_rolagem() {
            let mut app = app_com_texto(&"linha\n\n".repeat(30));
            app.view.scroll = 7;
            assert_eq!(app.ref_line(), 7);
            assert_eq!(app.window_lines(), (7, 17));
        }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv y_ 2>&1 | tail -10)`
Expected: erro de compilação (`pending_osc`, `ref_line`... não existem).

- [ ] **Step 3: Implementar**

Em `src/main.rs`:

1. `App` ganha `pending_osc: Option<String>,` (e `pending_osc: None,` em `App::new`).

2. Métodos novos em `impl App` (perto de `scroll_to`):

```rust
    /// Linha renderizada de referência para `y`/`Y`/`t`: a primeira visível.
    fn ref_line(&self) -> usize {
        self.view.scroll
    }

    /// Linhas renderizadas visíveis, `[topo, base)`.
    fn window_lines(&self) -> (usize, usize) {
        (self.view.scroll, self.view.scroll + self.view.height)
    }

    /// Rola para que a linha renderizada `line` fique no topo.
    fn scroll_to_line(&mut self, line: usize) {
        self.scroll_to(line);
    }

    /// Copia o bloco de código sob a linha de referência (`y`).
    fn copy_code(&mut self) {
        let (top, bottom) = (self.ref_line(), self.window_lines().1);
        let Some(text) = self.rendered.code_block_at(top, bottom).map(|c| c.text.clone()) else {
            self.notice = Some("nenhum bloco de código".to_string());
            return;
        };
        self.copy_text(&text);
    }

    /// Copia o Markdown cru da seção da linha de referência (`Y`).
    fn copy_section(&mut self) {
        if self.raw || self.doc.mode != cli::Mode::Markdown {
            self.notice = Some("seção só existe no Markdown renderizado".to_string());
            return;
        }
        let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) else {
            self.notice = Some("nenhuma seção aqui".to_string());
            return;
        };
        match render::section_source(&self.md, &self.rendered, idx) {
            Some(text) => self.copy_text(&text),
            None => self.notice = Some("não consegui recortar a seção".to_string()),
        }
    }

    /// Prepara o OSC 52 (escrito pelo `event_loop`) ou avisa que o texto é grande demais.
    fn copy_text(&mut self, text: &str) {
        match clip::osc52(text) {
            Ok(seq) => {
                self.pending_osc = Some(seq);
                let n = text.lines().count();
                self.notice = Some(format!("copiado: {n} linha{}", if n == 1 { "" } else { "s" }));
            }
            Err(bytes) => {
                self.notice = Some(format!("grande demais ({} KiB), nada copiado", bytes.div_ceil(1024)));
            }
        }
    }
```

3. Em `open_visible_table`, troque `let (top, bottom) = (self.view.scroll, self.view.scroll + self.view.height);` por `let (top, bottom) = self.window_lines();`.

4. Em `on_key`, acrescente os braços (perto de `KeyCode::Char('t')`):

```rust
            KeyCode::Char('y') => self.copy_code(),
            KeyCode::Char('Y') => self.copy_section(),
```

5. Função livre e `event_loop`:

```rust
/// Escreve uma sequência de controle direto no terminal (OSC 52).
fn emit_osc(seq: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}
```

No `event_loop`, logo depois de `if k.kind == KeyEventKind::Press && app.on_key(k) { return Ok(()); }` (dentro do `if let Event::Key`), acrescente:

```rust
                if let Some(seq) = app.pending_osc.take() {
                    emit_osc(&seq);
                }
```

6. Texto da barra: em `status_line`, troque `"j/k rolar  [ ] pular título  / buscar  t tabela  m fonte  q sair "` por `"j/k rolar  [ ] título  / buscar  t tabela  y/Y copiar  m fonte  q sair "` e ajuste o teste `barra_de_status_explica_que_as_teclas_pulam_de_titulo` se ele comparar a string antiga (rode a suíte e veja).

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -10`

```bash
git add src/main.rs
git commit -m "feat(tui): y copia o bloco de código e Y a seção via OSC 52 (F7.4)"
```

---

### Task 4: Documentação

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`

- [ ] **Step 1: README**

- Na lista de teclas do leitor (procure a linha de `t` de tabela), acrescente: ``- `y` copia o bloco de código da tela; `Y` copia o Markdown da seção atual (para o terminal, via OSC 52). Máximo de 64 KiB.``
- Em "Limitações conhecidas": "- `y`/`Y` dependem de o terminal aceitar OSC 52 (no tmux, `set -g set-clipboard on`); terminais sem suporte ignoram em silêncio."
- No bloco de ajuda (`<!-- mdv:help:start -->`), nada muda (as teclas não estão no `--help`).

- [ ] **Step 2: CLAUDE.md**

Em "Arquitetura", no item de `src/render.rs`, acrescente: "`Rendered` também guarda `line_src` (linha do fonte de cada linha renderizada, vazio fora do Markdown), `code_blocks` e `tasks`; `section_range`/`section_at`/`section_source` recortam seções." E depois do item de `src/mem.rs`: "- `src/clip.rs`: base64 e OSC 52 (`y`/`Y`); a TUI guarda a sequência em `App.pending_osc` e o `event_loop` a escreve."

- [ ] **Step 3: Spec e verificação**

Na spec, na seção F7.4, acrescente a linha: "Aviso de cópia dura até a próxima tecla (como os demais avisos da barra), não 2 s."

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED") && cargo clippy --all-targets 2>&1 | tail -5`
Expected: verde, sem avisos.

```bash
git add README.md CLAUDE.md docs/superpowers
git commit -m "docs: teclas y/Y e a fundação do Rendered (F7.4)"
```

---

## Self-Review

**Cobertura da spec:** `line_src`, `code_blocks`, `tasks`, `section_range` → Task 1. Alvo de `y` (bloco da linha de referência, senão o primeiro que cruza a janela, senão aviso) → `code_block_at` + `copy_code`. `Y` (Markdown cru da seção via `line_src`) → `section_source` + `copy_section`. OSC 52, base64 local, limite de 64 KiB, `sanitize` → Task 2. Feedback "copiado: N linhas" → Task 3 (duração: até a próxima tecla, registrada na spec; a spec dizia 2 s). Testes pedidos: vetores RFC 4648, escolha do alvo, limite, sequência emitida → Tasks 1–3.

**Placeholders:** nenhum. O único ponto condicional (linha do marcador de tarefa) traz a ação de contorno.

**Tipos:** `CodeSpan`, `TaskItem`, `section_range(&[OutlineEntry], usize, usize) -> (usize, usize)`, `section_at(&[OutlineEntry], usize) -> Option<usize>`, `section_source(&str, &Rendered, usize) -> Option<String>`, `osc52(&str) -> Result<String, usize>` e `ref_line`/`window_lines`/`scroll_to_line` têm a mesma assinatura em todas as tarefas e nos planos seguintes.
