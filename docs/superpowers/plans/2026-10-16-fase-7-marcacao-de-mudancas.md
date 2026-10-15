# F7.1 — Marcação de mudanças no reload: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Quando o arquivo aberto na TUI muda (live reload, `r` ou `--follow`), destacar na margem esquerda as linhas novas ou alteradas desde a versão anterior, com `}`/`{` para saltar entre elas e `c` para limpar.

**Architecture:** Um módulo puro `src/diff.rs` compara o fonte antigo e o novo com o crate `similar` e devolve as linhas **novas** (inserções e substituições); `marks` converte isso em um `Vec<bool>` por linha renderizada usando `Rendered.line_src` (fundação da F7.4). `App` guarda `changed` (linhas do fonte) e `marks` (por linha renderizada), recalcula `marks` a cada `rerender`, desenha `▌` amarelo numa coluna de margem fora da área do texto e tem as teclas `}`, `{`, `c`. O `--plain` não muda.

**Tech Stack:** Rust 2021, `similar` 2 (única dependência nova da Fase 7), `ratatui`.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md` (seção "F7.1 Marcação de mudanças no reload").

**Pré-requisitos:** `docs/superpowers/plans/2026-10-16-fase-7-fundacao-e-copiar.md` (Tasks 1 e 3: `Rendered.line_src`, `ref_line`, `scroll_to_line`) e `docs/superpowers/plans/2026-10-16-fase-7-fold-por-titulo.md` (`display`, `window_display`, `reveal`, `is_visible`).

## Global Constraints

- Mensagens e docs em português, com acentuação correta.
- Dependência nova permitida: somente `similar = "2"` (sem outras features além de `text`).
- Sem marcas na carga inicial. As marcas persistem até a tecla `c` ou o próximo reload (que as substitui; sempre relativas à versão imediatamente anterior).
- Só linhas **novas** (inserções e substituições) são marcadas; remoção pura não marca nada.
- Acima de 20 000 linhas (`MAX_DIFF_LINES`) no fonte antigo ou no novo, não calcula o diff: aviso "documento grande: sem marcação de mudanças".
- `--plain` e os modos JSON/CSV não mudam (sem `line_src`, nada é marcado).
- O diff tem prazo (300 ms) para não travar a TUI.
- Regra do `CLAUDE.md`: nenhum teste aloca memória sem teto (os testes de "documento grande" usam ~40 KiB); rode `(ulimit -v 6000000; cargo test 2>&1 | tail -20)`.
- `cargo clippy --all-targets` sem avisos novos.

## Review Focus

- Linha em branco entre dois parágrafos alterados: a margem não pinta a linha em branco, e cada trecho vira uma "mudança" para `}`/`{` (Task 2).
- Parágrafo longo quebrado pela largura em várias linhas renderizadas, com uma edição no meio do fonte: todas as linhas do bloco ficam marcadas (Task 1).
- Reload duplo (v1→v2→v3): só as mudanças de v2→v3 ficam marcadas; reload sem diferença limpa as marcas (Task 2).
- Documento que passa de 20 000 linhas: sem diff, sem marcas, aviso na barra, sem travar (Task 1 e 2).
- `--follow` com stdin: marcas só nas linhas que acabaram de chegar; o primeiro pedaço (documento vazio antes) não marca tudo (Task 2).

---

## File Structure

- `Cargo.toml` / `Cargo.lock`: dependência `similar`.
- `src/diff.rs` (novo): `MAX_DIFF_LINES`, `changed_lines`, `marks`.
- `src/main.rs`: `mod diff;`, estado e métodos de `App`, `set_md`/`append_md`, desenho da margem, teclas, barra.
- `README.md`, `CLAUDE.md`, spec: docs.

## Pontos do código atual que o plano usa

- `App::reload()` faz `self.md = md; self.mtime = mtime; self.dirty = true;` quando a leitura funciona. `App::drain_stream()` faz `self.md.push_str(&chunk)` para cada pedaço de stdin. `follow_latest` troca de arquivo atribuindo `self.md`.
- `App::rerender(w)` termina com `self.recompute_display();` (F7.3). `App::draw` calcula `inner` (área do texto, centralizada) e renderiza `screen_lines()`.
- `Rendered.line_src: Vec<usize>` (F7.4): vazio fora do Markdown; do mesmo comprimento de `lines` no Markdown.
- Helpers da F7.3: `display`, `window_display() -> &[usize]`, `is_visible`, `reveal`, `scroll_to_line`, `ref_line`.
- `line_text(&Line) -> String` em `main.rs`.

---

### Task 1: Módulo `diff` (puro) e dependência

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `src/main.rs` (`mod diff;`)
- Create: `src/diff.rs`
- Test: `src/diff.rs`

**Interfaces:**
- Consumes: nada.
- Produces (usadas pela Task 2):
  - `pub const MAX_DIFF_LINES: usize = 20_000`
  - `pub fn changed_lines(old: &str, new: &str) -> Option<BTreeSet<usize>>` (linhas 0-based de `new` que são inserções ou substituições; `None` se passa de `MAX_DIFF_LINES`)
  - `pub fn marks(line_src: &[usize], changed: &BTreeSet<usize>) -> Vec<bool>` (um `bool` por linha renderizada; vazio se `line_src` é vazio)

- [ ] **Step 1: Adicionar a dependência**

Em `Cargo.toml`, na seção `[dependencies]`:

```toml
similar = { version = "2", default-features = false, features = ["text"] }
```

Run: `cargo build 2>&1 | tail -3`
Expected: compila (baixa o crate). Confira com `cargo tree -p similar --depth 1` que não puxou dependências pesadas.

- [ ] **Step 2: Escrever os testes que falham**

Crie `src/diff.rs` só com:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[usize]) -> BTreeSet<usize> {
        v.iter().copied().collect()
    }

    #[test]
    fn insercao_marca_a_linha_nova() {
        assert_eq!(changed_lines("a\nb\n", "a\nx\nb\n").unwrap(), set(&[1]));
    }

    #[test]
    fn edicao_marca_a_linha_substituida() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nB\nc\n").unwrap(), set(&[1]));
    }

    #[test]
    fn remocao_pura_nao_marca_nada() {
        assert!(changed_lines("a\nb\nc\n", "a\nc\n").unwrap().is_empty());
    }

    #[test]
    fn texto_igual_nao_marca_nada() {
        assert!(changed_lines("a\nb\n", "a\nb\n").unwrap().is_empty());
    }

    #[test]
    fn acrescentar_no_fim_marca_as_linhas_novas() {
        assert_eq!(changed_lines("a\n", "a\nb\nc\n").unwrap(), set(&[1, 2]));
    }

    #[test]
    fn documento_grande_demais_nao_calcula() {
        let grande = "x\n".repeat(MAX_DIFF_LINES + 1);
        assert!(changed_lines(&grande, "x\n").is_none());
        assert!(changed_lines("x\n", &grande).is_none());
        let no_teto = "x\n".repeat(MAX_DIFF_LINES);
        assert!(changed_lines(&no_teto, &no_teto).is_some());
    }

    #[test]
    fn marks_pinta_todas_as_linhas_do_bloco_quebrado() {
        // Parágrafo da linha 0 do fonte quebrado em 3 linhas renderizadas; o seguinte começa na linha 3.
        let line_src = [0, 0, 0, 3, 3, 5];
        assert_eq!(marks(&line_src, &set(&[1])), vec![true, true, true, false, false, false]);
        assert_eq!(marks(&line_src, &set(&[4])), vec![false, false, false, true, true, false]);
    }

    #[test]
    fn marks_de_lista_aninhada_pinta_so_o_item() {
        assert_eq!(marks(&[0, 1, 2], &set(&[1])), vec![false, true, false]);
    }

    #[test]
    fn marks_sem_mudancas_ou_sem_mapa_e_tudo_falso_ou_vazio() {
        assert_eq!(marks(&[0, 1], &BTreeSet::new()), vec![false, false]);
        assert!(marks(&[], &set(&[0])).is_empty());
    }

    #[test]
    fn marks_aceita_linha_de_fonte_alem_do_fim_do_mapa() {
        assert_eq!(marks(&[0, 2], &set(&[9])), vec![false, true]);
    }
}
```

Em `src/main.rs`, acrescente `mod diff;` (depois de `mod data;`).

- [ ] **Step 3: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv diff:: 2>&1 | tail -8)`
Expected: erro de compilação (`changed_lines`, `marks`, `MAX_DIFF_LINES` não definidos).

- [ ] **Step 4: Implementar**

Acima do módulo `tests` de `src/diff.rs`:

```rust
//! Diferença entre duas versões do fonte, para marcar o que mudou no reload.

use std::collections::BTreeSet;
use std::time::Duration;

use similar::{Algorithm, ChangeTag, TextDiff};

/// Acima disso (em qualquer das versões) o diff não é calculado.
pub const MAX_DIFF_LINES: usize = 20_000;

/// Prazo do diff: passado dele, `similar` devolve um resultado grosseiro mas válido.
const DIFF_TIMEOUT: Duration = Duration::from_millis(300);

/// Linhas (0-based) de `new` que são inserções ou substituições em relação a `old`.
/// `None` se alguma das versões passa de `MAX_DIFF_LINES` linhas.
pub fn changed_lines(old: &str, new: &str) -> Option<BTreeSet<usize>> {
    if old.lines().count() > MAX_DIFF_LINES || new.lines().count() > MAX_DIFF_LINES {
        return None;
    }
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new);
    let mut out = BTreeSet::new();
    for change in diff.iter_all_changes() {
        if change.tag() == ChangeTag::Insert {
            out.extend(change.new_index());
        }
    }
    Some(out)
}

/// Uma marca por linha renderizada: a linha é marcada se alguma linha do fonte do seu bloco
/// (da `line_src` dela até a do próximo bloco) mudou.
pub fn marks(line_src: &[usize], changed: &BTreeSet<usize>) -> Vec<bool> {
    let n = line_src.len();
    if changed.is_empty() {
        return vec![false; n];
    }
    // `next[i]`: primeira linha do fonte, depois da linha renderizada `i`, maior que a dela.
    let mut next = vec![usize::MAX; n];
    for i in (0..n.saturating_sub(1)).rev() {
        next[i] = if line_src[i + 1] > line_src[i] { line_src[i + 1] } else { next[i + 1] };
    }
    (0..n).map(|i| changed.range(line_src[i]..next[i]).next().is_some()).collect()
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde. (`dead_code` do clippy em `diff.rs` até a Task 2 é esperado.)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/diff.rs src/main.rs
git commit -m "feat(diff): linhas novas entre duas versões do fonte, mapeadas para as linhas renderizadas (F7.1)"
```

---

### Task 2: Estado, margem, teclas e barra na TUI

**Files:**
- Modify: `src/main.rs`
- Test: `src/main.rs` (módulo `tests`, no submódulo com `app_com_texto` e `ev`)

**Interfaces:**
- Consumes: `diff::{changed_lines, marks, MAX_DIFF_LINES}` (Task 1); `ref_line`, `scroll_to_line`, `window_display`, `reveal`, `is_visible` (F7.3).
- Produces:
  - campos `App.changed: BTreeSet<usize>`, `App.marks: Vec<bool>`
  - `App::set_md(&mut self, new: String)`, `App::append_md(&mut self, chunk: &str)`
  - `App::recompute_marks(&mut self)`, `App::margin_lines(&self) -> Vec<Line<'static>>`, `App::change_runs(&self) -> usize`, `App::jump_change(&mut self, forward: bool)`

- [ ] **Step 1: Escrever os testes que falham**

```rust
        const V1: &str = "primeiro\n\nsegundo\n\nterceiro\n";

        fn linha_com(app: &App, trecho: &str) -> usize {
            (0..app.rendered.lines.len())
                .find(|&i| line_text(&app.rendered.lines[i]).contains(trecho))
                .unwrap_or_else(|| panic!("sem linha com {trecho:?}"))
        }

        fn marcada(app: &App, trecho: &str) -> bool {
            app.marks.get(linha_com(app, trecho)).copied().unwrap_or(false)
        }

        #[test]
        fn carga_inicial_nao_tem_marcas() {
            let app = app_com_texto(V1);
            assert!(app.changed.is_empty());
            assert!(app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn reload_marca_so_as_linhas_novas() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nsegundo EDITADO\n\nterceiro\n\nquarto\n".to_string());
            app.rerender(60);
            assert!(marcada(&app, "EDITADO") && marcada(&app, "quarto"));
            assert!(!marcada(&app, "primeiro") && !marcada(&app, "terceiro"));
        }

        #[test]
        fn linhas_em_branco_entre_blocos_nao_sao_pintadas() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nsegundo\n\nterceiro\n\nA\n\nB\n".to_string());
            app.rerender(60);
            for (i, m) in app.marks.iter().enumerate() {
                if *m {
                    assert!(!line_text(&app.rendered.lines[i]).trim().is_empty(), "linha {i} em branco pintada");
                }
            }
            assert_eq!(app.change_runs(), 2, "A e B são trechos separados pela linha em branco");
        }

        #[test]
        fn reload_duplo_so_marca_a_diferenca_para_a_versao_anterior() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nsegundo\n\nterceiro\n\nquarto\n".to_string());
            app.set_md("primeiro\n\nsegundo\n\nterceiro\n\nquarto\n\nquinto\n".to_string());
            app.rerender(60);
            assert!(marcada(&app, "quinto"));
            assert!(!marcada(&app, "quarto"), "quarto já estava na versão anterior");
        }

        #[test]
        fn reload_sem_diferenca_limpa_as_marcas() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            assert!(!app.changed.is_empty());
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            assert!(app.changed.is_empty() && app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn remocao_nao_marca_nada() {
            let mut app = app_com_texto(V1);
            app.set_md("primeiro\n\nterceiro\n".to_string());
            app.rerender(60);
            assert!(app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn paragrafo_quebrado_em_varias_linhas_fica_todo_marcado() {
            let longo = "palavra ".repeat(30);
            let mut app = app_com_texto(&format!("{longo}\n\nfim\n"));
            app.set_md(format!("{longo}\n{longo}\n\nfim\n"));
            app.rerender(30);
            let linhas: Vec<usize> = (0..app.rendered.lines.len())
                .filter(|&i| line_text(&app.rendered.lines[i]).contains("palavra"))
                .collect();
            assert!(linhas.len() > 2);
            assert!(linhas.iter().all(|&i| app.marks[i]));
            assert!(!marcada(&app, "fim"));
        }

        #[test]
        fn documento_grande_nao_calcula_diff_e_avisa() {
            let mut app = app_com_texto("a\n");
            app.set_md("x\n".repeat(diff::MAX_DIFF_LINES + 1));
            assert!(app.changed.is_empty());
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("documento grande")), "{:?}", app.notice);
            assert_eq!(app.md.lines().count(), diff::MAX_DIFF_LINES + 1, "o texto novo é adotado mesmo assim");
        }

        #[test]
        fn c_limpa_as_marcas() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            assert!(app.marks.iter().any(|&m| m));
            app.on_key(ev(KeyCode::Char('c')));
            assert!(app.changed.is_empty() && app.marks.iter().all(|&m| !m));
        }

        #[test]
        fn chaves_saltam_entre_trechos_alterados() {
            let base: String = (1..=12).map(|i| format!("par{i}\n\n")).collect();
            let mut app = app_com_texto(&base);
            app.view.height = 3;
            let novo = base.replace("par4\n", "par4 mudou\n").replace("par9\n", "par9 mudou\n");
            app.set_md(novo);
            app.rerender(60);
            app.view.scroll = 0;
            app.on_key(ev(KeyCode::Char('}')));
            assert_eq!(app.ref_line(), linha_com(&app, "par4 mudou"));
            app.on_key(ev(KeyCode::Char('}')));
            assert_eq!(app.ref_line(), linha_com(&app, "par9 mudou"));
            app.on_key(ev(KeyCode::Char('}')));
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("mais")), "{:?}", app.notice);
            app.on_key(ev(KeyCode::Char('{')));
            assert_eq!(app.ref_line(), linha_com(&app, "par4 mudou"));
        }

        #[test]
        fn salto_sem_marcas_avisa() {
            let mut app = app_com_texto(V1);
            app.on_key(ev(KeyCode::Char('}')));
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma mudança")));
        }

        #[test]
        fn margem_desenha_a_barra_amarela_nas_linhas_marcadas() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            let margem = app.margin_lines();
            let i = linha_com(&app, "novo") - app.view.scroll;
            assert_eq!(line_text(&margem[i]), "▌");
            assert_eq!(margem[i].spans[0].style.fg, Some(Color::Yellow));
            assert_eq!(line_text(&margem[0]), " ");
        }

        #[test]
        fn barra_de_status_conta_os_trechos_alterados() {
            let mut app = app_com_texto(V1);
            app.set_md(format!("{V1}\nnovo\n"));
            app.rerender(60);
            assert!(app.status_left().contains("1 mudança"), "{}", app.status_left());
        }

        #[test]
        fn recarregar_o_arquivo_marca_o_que_mudou() {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.md");
            std::fs::write(&p, V1).unwrap();
            let mut app = test_app(Source::File(p.clone()), V1);
            app.rerender(60);
            app.view.height = 10;
            std::fs::write(&p, format!("{V1}\nnovo\n")).unwrap();
            app.reload();
            app.rerender(60);
            assert!(marcada(&app, "novo"));
        }

        #[test]
        fn stream_marca_so_o_que_acabou_de_chegar_e_nao_o_primeiro_pedaco() {
            let mut app = test_app(Source::Stdin, "");
            app.append_md("a\n");
            assert!(app.changed.is_empty(), "documento vazio antes: nada de marcar tudo");
            app.append_md("b\nc\n");
            assert_eq!(app.changed.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
            app.append_md("d");
            assert_eq!(app.changed.iter().copied().collect::<Vec<_>>(), vec![3]);
            app.append_md("e\n");
            assert_eq!(app.changed.iter().copied().collect::<Vec<_>>(), vec![3], "a linha parcial foi completada");
        }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv carga_inicial_nao_tem_marcas 2>&1 | tail -8)`
Expected: erro de compilação (`changed`, `marks`, `set_md`... não existem).

- [ ] **Step 3: Implementar**

Em `src/main.rs`:

1. `use std::collections::{BTreeSet, HashMap, HashSet};`.

2. Campos em `App` e em `App::new`:

```rust
    changed: BTreeSet<usize>,
    marks: Vec<bool>,
```
```rust
            changed: BTreeSet::new(),
            marks: Vec::new(),
```

3. Métodos em `impl App`:

```rust
    /// Troca o texto do documento (reload) marcando as linhas novas em relação ao anterior.
    fn set_md(&mut self, new: String) {
        if self.md.is_empty() {
            self.changed.clear();
        } else {
            match diff::changed_lines(&self.md, &new) {
                Some(set) => self.changed = set,
                None => {
                    self.changed.clear();
                    self.notice = Some("documento grande: sem marcação de mudanças".to_string());
                }
            }
        }
        self.md = new;
        self.dirty = true;
    }

    /// Acrescenta um pedaço de stdin (`--follow`) marcando só as linhas que acabaram de chegar.
    fn append_md(&mut self, chunk: &str) {
        let first_new = self.md.matches('\n').count();
        let was_empty = self.md.is_empty();
        self.md.push_str(chunk);
        let total = self.md.matches('\n').count() + usize::from(!self.md.ends_with('\n'));
        if was_empty || total.saturating_sub(first_new) > diff::MAX_DIFF_LINES {
            self.changed.clear();
        } else {
            self.changed = (first_new..total).collect();
        }
    }

    /// Marcas por linha renderizada; linhas em branco nunca são pintadas.
    fn recompute_marks(&mut self) {
        let mut m = diff::marks(&self.rendered.line_src, &self.changed);
        for (i, mark) in m.iter_mut().enumerate() {
            if *mark && self.rendered.lines.get(i).is_none_or(|l| line_text(l).trim().is_empty()) {
                *mark = false;
            }
        }
        self.marks = m;
    }

    fn is_marked(&self, line: usize) -> bool {
        self.marks.get(line).copied().unwrap_or(false)
    }

    /// Quantos trechos contíguos marcados existem.
    fn change_runs(&self) -> usize {
        (0..self.marks.len()).filter(|&i| self.is_marked(i) && (i == 0 || !self.is_marked(i - 1))).count()
    }

    /// A coluna de margem da janela: `▌` amarelo nas linhas marcadas.
    fn margin_lines(&self) -> Vec<Line<'static>> {
        self.window_display()
            .iter()
            .map(|&l| {
                if self.is_marked(l) {
                    Line::from(Span::styled("▌", Style::default().fg(Color::Yellow)))
                } else {
                    Line::from(" ")
                }
            })
            .collect()
    }

    /// `}` / `{`: vai ao início do próximo / anterior trecho marcado.
    fn jump_change(&mut self, forward: bool) {
        if self.change_runs() == 0 {
            self.notice = Some("nenhuma mudança marcada".to_string());
            return;
        }
        let top = self.ref_line();
        let starts = (0..self.marks.len()).filter(|&i| self.is_marked(i) && (i == 0 || !self.is_marked(i - 1)));
        let alvo = if forward { starts.into_iter().find(|&i| i > top) } else { starts.into_iter().filter(|&i| i < top).last() };
        match alvo {
            Some(line) => {
                self.reveal(line);
                self.scroll_to_line(line);
            }
            None => {
                self.notice = Some(if forward { "sem mais mudanças adiante" } else { "sem mais mudanças atrás" }.to_string());
            }
        }
    }
```

(Se `Option::is_none_or` não existir na versão do Rust do projeto, troque por `self.rendered.lines.get(i).map_or(true, |l| ...)`.)

4. Chamadas: `rerender` termina com `self.recompute_display();` → acrescente depois `self.recompute_marks();`. `reload()`: troque `self.md = md; self.mtime = mtime; self.dirty = true;` por `self.set_md(md); self.mtime = mtime;`. `drain_stream`: troque `self.md.push_str(&chunk);` por `self.append_md(&chunk);`. `follow_latest`: depois de `self.md = md;` acrescente `self.changed.clear();` (é outro documento).

5. Teclas em `on_key`:

```rust
            KeyCode::Char('}') => self.jump_change(true),
            KeyCode::Char('{') => self.jump_change(false),
            KeyCode::Char('c') => {
                self.changed.clear();
                self.marks.clear();
            }
```

6. `draw`: depois de `f.render_widget(Paragraph::new(visible), inner);` (o `visible` vem de `screen_lines()` na F7.3), acrescente:

```rust
        if !self.marks.is_empty() && inner.x > content.x {
            let margem = Rect { x: inner.x - 1, y: inner.y, width: 1, height: inner.height };
            f.render_widget(Paragraph::new(self.margin_lines()), margem);
        }
```

7. `status_left`, antes de `if let Some(n) = &self.notice`:

```rust
        let runs = self.change_runs();
        if runs > 0 {
            let _ = write!(s, "  ▌ {runs} mudança{} (c limpa)", if runs == 1 { "" } else { "s" });
        }
```

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde. Ajustes prováveis: o teste do stream espera o modelo "linha parcial": `"d"` (sem `\n`) → `total = 3 + 1 = 4`, `first_new = 3` → `{3}`; depois `"e\n"` → `first_new = 3`, total 4 → `{3}`.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/main.rs
git commit -m "feat(tui): marca na margem o que mudou no reload, com } { e c (F7.1)"
```

---

### Task 3: Documentação

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `PLANO.md`, `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`

- [ ] **Step 1: README**

- Lista de teclas: "`}` / `{` saltam para a próxima / anterior mudança do reload; `c` limpa as marcas."
- Em "Recarga automática" (ou onde o live reload é descrito), acrescente: "Quando o arquivo muda, as linhas novas ou alteradas ganham uma barra amarela `▌` na margem esquerda, relativa à versão imediatamente anterior. Remoções não são marcadas. Acima de 20 000 linhas o `mdv` não calcula a diferença. No `--plain` não há marcas."

- [ ] **Step 2: CLAUDE.md, PLANO.md, spec**

- `CLAUDE.md`, "Arquitetura": "- `src/diff.rs`: `changed_lines` (crate `similar`, prazo de 300 ms, teto de 20 000 linhas) e `marks` (linhas renderizadas via `Rendered.line_src`). `App::set_md`/`append_md` são os únicos pontos que trocam `md` no reload e no `--follow`; `App.marks` é recalculado em `rerender`."
- `PLANO.md` (gitignored): marque `[x] F7.1`.
- Spec, F7.1: "Linhas em branco nunca recebem a barra, e cada trecho contíguo marcado conta como uma mudança para `}`/`{`. Remoção pura não marca nada. No `--follow` com stdin, a marcação cobre só as linhas do último pedaço recebido (o primeiro pedaço não marca). `}`/`{` abrem a dobra que esconde o destino (F7.3)."

- [ ] **Step 3: Verificação final e commit**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED") && cargo clippy --all-targets 2>&1 | tail -5`
Expected: verde, sem avisos.

Validação manual (precisa de terminal): `cargo run --release -- /tmp/x.md`; em outro terminal `echo '\nnovo' >> /tmp/x.md` e veja a barra amarela; `}` e `c`.

```bash
git add README.md CLAUDE.md docs/superpowers
git commit -m "docs: marcação de mudanças no reload (F7.1)"
```

---

## Self-Review

**Cobertura da spec:** `set_md` com `similar` e `changed` das linhas novas → Tasks 1–2 (`set_md` e `append_md` para o `--follow`). Marca `▌` amarela na margem esquerda sem tocar `render.rs` → `margin_lines` e `draw`. `--plain` não muda → nada tocado. Sem marcas na carga inicial → `carga_inicial_nao_tem_marcas`. Marcas persistem até `c` ou o próximo reload → `reload_duplo...`, `c_limpa_as_marcas`. `}`/`{` → `jump_change`. Limite de 20 000 linhas com aviso → `documento_grande_...`. Testes pedidos: inserção, edição, remoção, reload duplo, mapeamento via `line_src` com linha quebrada em duas, `c`, salto → Tasks 1–2.

**Placeholders:** nenhum. Único ponto condicional: `Option::is_none_or` (a alternativa está escrita).

**Tipos:** `changed_lines(&str, &str) -> Option<BTreeSet<usize>>`, `marks(&[usize], &BTreeSet<usize>) -> Vec<bool>`, `App.changed: BTreeSet<usize>`, `App.marks: Vec<bool>` e os métodos coincidem entre a Task 1, a Task 2 e os testes; `ref_line`, `scroll_to_line`, `window_display`, `reveal` vêm da F7.4/F7.3 com as mesmas assinaturas.

**Review Focus → testes:** linhas em branco e trechos (`linhas_em_branco_entre_blocos_nao_sao_pintadas`); parágrafo quebrado (`paragrafo_quebrado_em_varias_linhas_fica_todo_marcado`, `marks_pinta_todas_as_linhas_do_bloco_quebrado`); reload duplo e sem diferença (`reload_duplo_so_marca_a_diferenca_para_a_versao_anterior`, `reload_sem_diferenca_limpa_as_marcas`); documento grande (`documento_grande_demais_nao_calcula`, `documento_grande_nao_calcula_diff_e_avisa`); stream (`stream_marca_so_o_que_acabou_de_chegar_e_nao_o_primeiro_pedaco`).
