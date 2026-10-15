# F7.3 — Fold por título (`z` / `Z`): Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dobrar e expandir seções do documento na TUI: `z` alterna a seção da linha de referência, `Z` dobra todas (ou abre todas se já estiverem dobradas). O título dobrado ganha o sufixo ` ▸ N linhas`.

**Architecture:** Um módulo puro novo, `src/fold.rs`, calcula a lista `display` de linhas renderizadas visíveis a partir de `Rendered.outline` e do conjunto de dobras. A chave de uma dobra é `(título, n-ésima ocorrência)`, estável em reload e mudança de largura. `App` guarda `folded`, `display` e `fold_hidden`; o `scroll` do `Viewport` passa a indexar `display`. A conversão fica atrás de `total_lines`, `ref_line`, `window_lines`, `scroll_to_line` (os helpers criados na F7.4), de modo que o resto do código não muda. A busca abre a dobra que esconde a ocorrência.

**Tech Stack:** Rust 2021. Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md` (seção "F7.3 Fold por título").

**Pré-requisito:** `docs/superpowers/plans/2026-10-16-fase-7-fundacao-e-copiar.md` executado até a Task 3 (`section_range`, `section_at`, `App::ref_line/window_lines/scroll_to_line`, helpers de teste `app_com_texto`/`ev`). A F7.2 é opcional (o contador de tarefas passa a usar `ref_line` automaticamente).

## Global Constraints

- Mensagens e docs em português, com acentuação correta.
- Chave de dobra = `(título, índice da ocorrência do mesmo título)`; nunca índice de linha (sobrevive a reload e a nova largura).
- `z` age na seção que contém a linha de referência (a primeira linha visível); `Z` dobra todas se alguma está aberta, senão abre todas.
- Busca (`/`, `n`, `N`) varre todas as linhas; saltar para uma ocorrência escondida abre a dobra.
- `]`/`[` pulam só entre títulos visíveis.
- `--plain` e os modos JSON/CSV não mudam (sem títulos, `display` é a identidade).
- Efeito aceito pela spec: `Viewport.scroll` passa a indexar `display`, não `lines`.
- Regra do `CLAUDE.md`: nenhum teste aloca memória sem teto; rode `(ulimit -v 6000000; cargo test 2>&1 | tail -20)`.
- `cargo clippy --all-targets` sem avisos novos.

## Review Focus

- Dobrar um título dentro de um título já dobrado, e abrir o pai: o filho continua dobrado (Task 1 e 3).
- Dois títulos com o mesmo texto: dobrar um não dobra o outro (Task 1).
- Reload ou redimensionamento com seções dobradas: continuam dobradas; título que sumiu do arquivo não quebra (Task 2).
- `t`, `y`, `Y` e `]`/`[` com seções dobradas na tela: nunca agem em conteúdo escondido (Task 2).
- `z`/`Z` em documento sem títulos, no modo fonte (`m`), em JSON e em CSV: aviso na barra, sem pânico (Task 3).

---

## File Structure

- `src/fold.rs` (novo): `FoldKey`, `fold_keys`, `Display`, `compute`.
- `src/main.rs`: `mod fold;`, estado e mapeamentos em `App`, desenho, navegação, busca, teclas `z`/`Z`, ajuda na barra.
- `README.md`, `CLAUDE.md`, spec: docs.

## Pontos do código atual que o plano usa

- `Rendered.outline: Vec<OutlineEntry { line, level, title }>`, `Rendered.headings: Vec<usize>`, `section_range`, `section_at` (F7.4).
- `App::total_lines()` (= `rendered.lines.len()`), `scroll_by`, `scroll_to`, `ref_line()`, `window_lines()`, `scroll_to_line()` (F7.4; hoje identidade sobre `view.scroll`), `rerender(&mut self, w)`, `goto_match`, `set_search`, `open_visible_table`, `copy_code`, `draw`.
- `on_key` trata `]`/`[` com `self.rendered.headings` e `self.view.scroll`.
- `draw` monta `visible` a partir de `self.rendered.lines[start..end]` com `highlight_line` quando há busca.

---

### Task 1: Módulo `fold` (puro)

**Files:**
- Create: `src/fold.rs`
- Modify: `src/main.rs` (`mod fold;`)
- Test: `src/fold.rs`

**Interfaces:**
- Consumes: `render::{section_range, OutlineEntry, Rendered}`.
- Produces (usadas pelas Tasks 2–3):
  - `pub type FoldKey = (String, usize);`
  - `pub fn fold_keys(outline: &[OutlineEntry]) -> Vec<FoldKey>` (uma chave por entrada do `outline`, na ordem)
  - `pub struct Display { pub lines: Vec<usize>, pub hidden: HashMap<usize, usize> }` (`lines` = índices visíveis em `Rendered.lines`; `hidden[l]` = linhas escondidas sob o título dobrado da linha `l`)
  - `pub fn compute(r: &Rendered, folded: &HashSet<FoldKey>) -> Display`

- [ ] **Step 1: Escrever os testes que falham**

Crie `src/fold.rs` só com:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{render, section_range};

    const DOC: &str = "# A\n\ntexto A\n\n## B\n\ntexto B\n\n## C\n\ntexto C\n\n# D\n\ntexto D\n";

    fn doc() -> Rendered {
        render(DOC, 40)
    }

    fn pastas(r: &Rendered, titulos: &[&str]) -> HashSet<FoldKey> {
        let keys = fold_keys(&r.outline);
        r.outline
            .iter()
            .zip(keys)
            .filter(|(e, _)| titulos.contains(&e.title.as_str()))
            .map(|(_, k)| k)
            .collect()
    }

    #[test]
    fn chaves_numeram_as_ocorrencias_do_mesmo_titulo() {
        let r = render("# A\n\n## x\n\n## x\n\n# x\n", 40);
        assert_eq!(
            fold_keys(&r.outline),
            vec![("A".to_string(), 0), ("x".to_string(), 0), ("x".to_string(), 1), ("x".to_string(), 2)]
        );
    }

    #[test]
    fn chave_sobrevive_a_edicao_em_outro_lugar() {
        let antes = render("# A\n\ntexto\n\n## B\n", 40);
        let depois = render("# A\n\ntexto\n\nmais um parágrafo\n\n## B\n", 40);
        assert_eq!(fold_keys(&antes.outline)[1], fold_keys(&depois.outline)[1]);
    }

    #[test]
    fn sem_dobras_todas_as_linhas_aparecem() {
        let r = doc();
        let d = compute(&r, &HashSet::new());
        assert_eq!(d.lines, (0..r.lines.len()).collect::<Vec<_>>());
        assert!(d.hidden.is_empty());
    }

    #[test]
    fn dobrar_um_titulo_esconde_o_corpo_e_mantem_o_titulo() {
        let r = doc();
        let d = compute(&r, &pastas(&r, &["B"]));
        let (s, e) = section_range(&r.outline, 1, r.lines.len());
        assert!(d.lines.contains(&s), "o título dobrado continua visível");
        assert!((s + 1..e).all(|l| !d.lines.contains(&l)));
        assert_eq!(d.hidden.get(&s), Some(&(e - s - 1)));
        let (cs, _) = section_range(&r.outline, 2, r.lines.len());
        assert!(d.lines.contains(&cs), "a seção irmã não é afetada");
    }

    #[test]
    fn dobrar_o_pai_esconde_os_filhos_e_so_o_pai_leva_o_sufixo() {
        let r = doc();
        let d = compute(&r, &pastas(&r, &["A", "B"]));
        let (a_s, a_e) = section_range(&r.outline, 0, r.lines.len());
        assert!((a_s + 1..a_e).all(|l| !d.lines.contains(&l)));
        assert!(d.hidden.contains_key(&a_s));
        assert!(!d.hidden.contains_key(&r.outline[1].line), "B está escondido; sem sufixo");
        let (d_s, _) = section_range(&r.outline, 3, r.lines.len());
        assert!(d.lines.contains(&d_s), "o título seguinte de mesmo nível continua visível");
    }

    #[test]
    fn abrir_o_pai_deixa_o_filho_dobrado() {
        let r = doc();
        let d = compute(&r, &pastas(&r, &["B"]));
        let (b_s, b_e) = section_range(&r.outline, 1, r.lines.len());
        assert!(d.lines.contains(&r.outline[0].line));
        assert!((b_s + 1..b_e).all(|l| !d.lines.contains(&l)), "B continua dobrado");
    }

    #[test]
    fn chave_de_titulo_que_sumiu_e_ignorada() {
        let r = doc();
        let mut f = HashSet::new();
        f.insert(("não existe".to_string(), 0));
        assert_eq!(compute(&r, &f).lines.len(), r.lines.len());
    }

    #[test]
    fn dois_titulos_iguais_dobram_separados() {
        let r = render("# x\n\na\n\n# x\n\nb\n", 40);
        let mut f = HashSet::new();
        f.insert(fold_keys(&r.outline)[1].clone());
        let d = compute(&r, &f);
        assert!(d.lines.contains(&r.outline[0].line) && d.lines.contains(&(r.outline[0].line + 2)));
        assert!(d.hidden.contains_key(&r.outline[1].line));
        assert!(!d.hidden.contains_key(&r.outline[0].line));
    }

    #[test]
    fn documento_sem_titulos_nao_muda() {
        let r = render("só texto\n\noutro\n", 40);
        let d = compute(&r, &HashSet::new());
        assert_eq!(d.lines.len(), r.lines.len());
    }
}
```

Em `src/main.rs`, acrescente `mod fold;` (depois de `mod extract;` se existir, senão depois de `mod data;`).

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv fold:: 2>&1 | tail -10)`
Expected: erro de compilação (`fold_keys`, `compute`... não definidos).

- [ ] **Step 3: Implementar**

Acima do módulo `tests` de `src/fold.rs`:

```rust
//! Dobra de seções: quais linhas renderizadas aparecem quando alguns títulos estão dobrados.

use std::collections::{HashMap, HashSet};

use crate::render::{section_range, OutlineEntry, Rendered};

/// (título, n-ésima ocorrência do mesmo título): estável em reload e mudança de largura.
pub type FoldKey = (String, usize);

/// Uma chave por entrada de `outline`, na mesma ordem.
pub fn fold_keys(outline: &[OutlineEntry]) -> Vec<FoldKey> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    outline
        .iter()
        .map(|e| {
            let n = seen.entry(e.title.as_str()).or_insert(0);
            let key = (e.title.clone(), *n);
            *n += 1;
            key
        })
        .collect()
}

/// Linhas visíveis (índices em `Rendered.lines`) e, para cada título dobrado visível,
/// quantas linhas ele esconde.
pub struct Display {
    pub lines: Vec<usize>,
    pub hidden: HashMap<usize, usize>,
}

pub fn compute(r: &Rendered, folded: &HashSet<FoldKey>) -> Display {
    let n = r.lines.len();
    let mut hidden_line = vec![false; n];
    let mut hidden = HashMap::new();
    if !folded.is_empty() {
        for (i, key) in fold_keys(&r.outline).iter().enumerate() {
            let line = r.outline[i].line;
            if !folded.contains(key) || line >= n || hidden_line[line] {
                continue;
            }
            let (start, end) = section_range(&r.outline, i, n);
            let end = end.min(n);
            if end > start + 1 {
                hidden_line[start + 1..end].fill(true);
                hidden.insert(start, end - start - 1);
            }
        }
    }
    let lines = (0..n).filter(|&l| !hidden_line[l]).collect();
    Display { lines, hidden }
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde. Se o clippy acusar `dead_code` em `fold.rs` até a Task 2, é esperado.

- [ ] **Step 5: Commit**

```bash
git add src/fold.rs src/main.rs
git commit -m "feat(fold): linhas visíveis com seções dobradas, chave estável por título (F7.3)"
```

---

### Task 2: `display` no `App` (sem mudar comportamento) e consumidores cientes de dobra

**Files:**
- Modify: `src/main.rs`
- Test: `src/main.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `fold::{compute, fold_keys, FoldKey}` (Task 1); `section_range`, `section_at`.
- Produces (usadas pela Task 3 e pelo plano F7.1):
  - campos `App.folded: HashSet<fold::FoldKey>`, `App.display: Vec<usize>`, `App.fold_hidden: HashMap<usize, usize>`
  - `App::recompute_display(&mut self)`
  - `App::line_at(&self, pos: usize) -> usize` (posição em `display` → linha renderizada; além do fim, `rendered.lines.len()`)
  - `App::pos_of(&self, line: usize) -> usize` (primeira posição de `display` com linha ≥ `line`)
  - `App::is_visible(&self, line: usize) -> bool`
  - `App::reveal(&mut self, line: usize)` (abre as dobras que escondem `line`)
  - `App::screen_lines(&self) -> Vec<Line<'static>>` (as linhas da janela, com destaque da busca e sufixo de dobra)
  - `total_lines`, `ref_line`, `window_lines`, `scroll_to_line` passam a usar `display`.

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/main.rs`, no submódulo com `app_com_texto` e `ev`:

```rust
        const DOC_FOLD: &str = "# A\n\ntexto A\n\n## A1\n\ntexto A1\n\n# B\n\ntexto B\n";

        fn dobrar(app: &mut App, titulo: &str) {
            let keys = fold::fold_keys(&app.rendered.outline);
            let i = app.rendered.outline.iter().position(|e| e.title == titulo).unwrap();
            app.folded.insert(keys[i].clone());
            app.recompute_display();
        }

        #[test]
        fn sem_dobras_display_e_identidade_e_nada_muda() {
            let app = app_com_texto(DOC_FOLD);
            assert_eq!(app.display, (0..app.rendered.lines.len()).collect::<Vec<_>>());
            assert_eq!(app.total_lines(), app.rendered.lines.len());
            assert_eq!(app.line_at(3), 3);
            assert_eq!(app.pos_of(3), 3);
        }

        #[test]
        fn dobra_encurta_o_documento_e_os_mapeamentos_seguem() {
            let mut app = app_com_texto(DOC_FOLD);
            let total = app.rendered.lines.len();
            dobrar(&mut app, "A");
            assert!(app.total_lines() < total);
            let b = app.rendered.outline.iter().find(|e| e.title == "B").unwrap().line;
            assert_eq!(app.line_at(app.pos_of(b)), b, "ida e volta num título visível");
            assert_eq!(app.line_at(usize::MAX), total, "além do fim: o total de linhas");
            assert!(app.is_visible(b) && !app.is_visible(b - 1));
        }

        #[test]
        fn ref_line_e_window_lines_usam_posicoes_do_display() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            let b = app.rendered.outline.iter().find(|e| e.title == "B").unwrap().line;
            app.view.scroll = app.pos_of(b);
            assert_eq!(app.ref_line(), b);
            app.scroll_to_line(0);
            assert_eq!(app.view.scroll, 0);
        }

        #[test]
        fn tela_mostra_o_sufixo_no_titulo_dobrado() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            let texto: Vec<String> = app.screen_lines().iter().map(line_text).collect();
            assert!(texto[0].contains("A") && texto[0].contains("▸") && texto[0].contains("linhas"), "{texto:?}");
            assert!(!texto.iter().any(|l| l.contains("texto A1")));
            assert!(texto.iter().any(|l| l.contains("texto B")));
        }

        #[test]
        fn colchetes_pulam_so_entre_titulos_visiveis() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.on_key(ev(KeyCode::Char(']')));
            let b = app.rendered.outline.iter().find(|e| e.title == "B").unwrap().line;
            assert_eq!(app.ref_line(), b, "A1 está escondido: o próximo título visível é B");
            app.on_key(ev(KeyCode::Char('[')));
            assert_eq!(app.ref_line(), 0);
        }

        #[test]
        fn busca_numa_secao_dobrada_abre_a_dobra() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.set_search("texto A1".to_string());
            assert!(app.folded.is_empty(), "a dobra que escondia a ocorrência foi aberta");
            let linha = app.search.current_line().unwrap();
            assert!(app.is_visible(linha));
        }

        #[test]
        fn busca_so_abre_a_dobra_que_esconde_a_linha() {
            let mut app = app_com_texto("# A\n\nx\n\n# B\n\ny\n");
            dobrar(&mut app, "A");
            dobrar(&mut app, "B");
            app.set_search("y".to_string());
            assert_eq!(app.folded.len(), 1, "A continua dobrada");
        }

        #[test]
        fn dobras_sobrevivem_a_reload_e_a_nova_largura() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.md = format!("{DOC_FOLD}\nmais um parágrafo\n");
            app.dirty = true;
            app.rerender(40);
            assert!(!app.display.iter().any(|&l| linha_contem(&app, l, "texto A1")));
        }

        #[test]
        fn dobra_de_titulo_que_sumiu_nao_quebra() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A");
            app.md = "# Outro\n\nx\n".to_string();
            app.rerender(60);
            assert_eq!(app.total_lines(), app.rendered.lines.len());
        }

        fn linha_contem(app: &App, l: usize, trecho: &str) -> bool {
            line_text(&app.rendered.lines[l]).contains(trecho)
        }

        #[test]
        fn t_nao_abre_tabela_escondida_numa_secao_dobrada() {
            let mut app = app_com_texto(&format!("# A\n\n{TABELA}\n# B\n\ntexto\n"));
            dobrar(&mut app, "A");
            app.on_key(ev(KeyCode::Char('t')));
            assert!(app.zoom.is_none());
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma tabela")));
        }

        #[test]
        fn y_nao_copia_bloco_de_codigo_escondido() {
            let mut app = app_com_texto("# A\n\n```\nsegredo\n```\n\n# B\n\ntexto\n");
            dobrar(&mut app, "A");
            app.on_key(ev(KeyCode::Char('y')));
            assert_eq!(app.pending_osc, None);
            assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhum bloco")));
        }
```

(`TABELA` é a constante já existente no submódulo de zoom; se estiver fora do escopo, copie `const TABELA: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";` para este submódulo.)

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv sem_dobras_display 2>&1 | tail -10)`
Expected: erro de compilação (`folded`, `display`, `recompute_display`... não existem).

- [ ] **Step 3: Implementar**

Em `src/main.rs`:

1. `use std::collections::{HashMap, HashSet};` (junto dos outros `use std`).

2. Campos em `App` e em `App::new`:

```rust
    folded: HashSet<fold::FoldKey>,
    display: Vec<usize>,
    fold_hidden: HashMap<usize, usize>,
```
```rust
            folded: HashSet::new(),
            display: Vec::new(),
            fold_hidden: HashMap::new(),
```

3. Métodos (substituem `total_lines`, `ref_line`, `window_lines`, `scroll_to_line` e acrescentam os novos):

```rust
    fn total_lines(&self) -> usize {
        self.display.len()
    }

    /// Recalcula quais linhas aparecem (depois de renderizar ou de dobrar).
    fn recompute_display(&mut self) {
        let d = fold::compute(&self.rendered, &self.folded);
        self.display = d.lines;
        self.fold_hidden = d.hidden;
        self.view.scroll = self.view.scroll.min(self.view.max_scroll(self.display.len()));
    }

    /// Posição de `display` → linha renderizada; além do fim, o total de linhas.
    fn line_at(&self, pos: usize) -> usize {
        self.display.get(pos).copied().unwrap_or(self.rendered.lines.len())
    }

    /// Primeira posição de `display` cuja linha é `line` ou vem depois.
    fn pos_of(&self, line: usize) -> usize {
        self.display.partition_point(|&l| l < line)
    }

    fn is_visible(&self, line: usize) -> bool {
        self.display.binary_search(&line).is_ok()
    }

    /// Linha renderizada de referência para `y`/`Y`/`z`/`t`: a primeira visível.
    fn ref_line(&self) -> usize {
        self.line_at(self.view.scroll)
    }

    /// Linhas renderizadas visíveis, `[topo, base)` (pode incluir linhas dobradas no meio).
    fn window_lines(&self) -> (usize, usize) {
        (self.line_at(self.view.scroll), self.line_at(self.view.scroll + self.view.height))
    }

    /// Posições de `display` que cabem na janela.
    fn window_display(&self) -> &[usize] {
        let a = self.view.scroll.min(self.display.len());
        let b = (self.view.scroll + self.view.height).min(self.display.len());
        &self.display[a..b]
    }

    /// Rola para que a linha renderizada `line` (ou a próxima visível) fique no topo.
    fn scroll_to_line(&mut self, line: usize) {
        self.scroll_to(self.pos_of(line));
    }

    /// Abre as dobras que escondem `line` (busca, saltos).
    fn reveal(&mut self, line: usize) {
        let n = self.rendered.lines.len();
        let keys = fold::fold_keys(&self.rendered.outline);
        let mut changed = false;
        for (i, key) in keys.iter().enumerate() {
            let (start, end) = render::section_range(&self.rendered.outline, i, n);
            if start < line && line < end {
                changed |= self.folded.remove(key);
            }
        }
        if changed {
            self.recompute_display();
        }
    }

    /// As linhas da janela como serão desenhadas: destaque da busca e sufixo nos títulos dobrados.
    fn screen_lines(&self) -> Vec<Line<'static>> {
        let q = self.search.query.to_lowercase();
        self.window_display()
            .iter()
            .map(|&i| {
                let l = &self.rendered.lines[i];
                let mut l = if q.is_empty() { l.clone() } else { highlight_line(l, &q) };
                if let Some(n) = self.fold_hidden.get(&i) {
                    l.spans.push(Span::styled(format!(" ▸ {n} linhas"), Style::default().fg(Color::Rgb(110, 118, 129))));
                }
                l
            })
            .collect()
    }
```

4. `rerender`: no fim do corpo, acrescente `self.recompute_display();`.

5. `draw`: troque o trecho de `let start = self.view.scroll;` até `f.render_widget(Paragraph::new(visible), inner);` por

```rust
        let visible = self.screen_lines();
        f.render_widget(Paragraph::new(visible), inner);
```
(o `end`/`q` locais deixam de existir; remova-os).

6. `set_search`/`goto_match`:

```rust
    fn set_search(&mut self, query: String) {
        self.search.commit(query, &self.rendered.lines, self.ref_line());
        self.goto_match();
    }
```
```rust
    fn goto_match(&mut self) {
        if let Some(line) = self.search.current_line() {
            self.reveal(line);
            let pos = self.pos_of(line);
            self.scroll_to(pos.saturating_sub(self.view.height / 3));
        }
    }
```

7. `]` e `[` em `on_key`:

```rust
            KeyCode::Char(']') => {
                let top = self.ref_line();
                let alvo = self.rendered.headings.iter().copied().find(|&h| h > top && self.is_visible(h));
                if let Some(h) = alvo {
                    self.scroll_to_line(h);
                }
            }
            KeyCode::Char('[') => {
                let top = self.ref_line();
                let alvo = self.rendered.headings.iter().rev().copied().find(|&h| h < top && self.is_visible(h));
                if let Some(h) = alvo {
                    self.scroll_to_line(h);
                }
            }
```

8. `open_visible_table`:

```rust
        fn open_visible_table(&mut self) {
            let janela = self.window_display();
            let achada = self.rendered.tables.iter().position(|t| janela.iter().any(|&l| l >= t.start && l < t.end));
            match achada {
                Some(i) => self.open_zoom(i),
                None => self.notice = Some("nenhuma tabela na tela".to_string()),
            }
        }
```

9. `copy_code` (da F7.4):

```rust
    fn copy_code(&mut self) {
        let top = self.ref_line();
        let janela = self.window_display();
        let achado = self
            .rendered
            .code_blocks
            .iter()
            .find(|c| c.start <= top && top < c.end)
            .or_else(|| self.rendered.code_blocks.iter().find(|c| janela.iter().any(|&l| l >= c.start && l < c.end)))
            .map(|c| c.text.clone());
        let Some(text) = achado else {
            self.notice = Some("nenhum bloco de código".to_string());
            return;
        };
        self.copy_text(&text);
    }
```

(Isso substitui o uso de `Rendered::code_block_at` em `copy_code`; a função de `render.rs` e o teste dela permanecem.)

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde, inclusive os testes antigos de zoom, busca e streaming (sem dobras, `display` é a identidade). Se algum teste antigo atribui `app.rendered = ...` direto e depois usa `total_lines()`, chame `app.recompute_display()` nele.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/main.rs
git commit -m "refactor(tui): rolagem sobre display, com busca, títulos, t e y cientes de dobra (F7.3)"
```

---

### Task 3: Teclas `z` e `Z`, ajuda e documentação

**Files:**
- Modify: `src/main.rs`, `README.md`, `CLAUDE.md`, `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`
- Test: `src/main.rs` (módulo `tests`)

**Interfaces:**
- Consumes: tudo da Task 2; `fold::fold_keys`; `render::{section_at, section_range}`.
- Produces: `App::toggle_fold`, `App::toggle_fold_all`.

- [ ] **Step 1: Escrever os testes que falham**

```rust
        #[test]
        fn z_dobra_a_secao_da_linha_de_referencia_e_de_novo_abre() {
            let mut app = app_com_texto("# A\n\na\n\n# B\n\nb\n");
            let total = app.total_lines();
            app.on_key(ev(KeyCode::Char('z')));
            assert!(app.total_lines() < total);
            assert_eq!(app.ref_line(), 0, "o título dobrado fica no topo");
            app.on_key(ev(KeyCode::Char('z')));
            assert_eq!(app.total_lines(), total);
            assert!(app.folded.is_empty());
        }

        #[test]
        fn z_age_no_titulo_mais_fundo_da_linha_de_referencia() {
            let mut app = app_com_texto(DOC_FOLD);
            let a1 = app.rendered.outline.iter().find(|e| e.title == "A1").unwrap().line;
            app.view.scroll = app.pos_of(a1);
            app.on_key(ev(KeyCode::Char('z')));
            assert_eq!(app.folded.len(), 1);
            assert!(app.is_visible(0), "o título A continua aberto");
            assert!(!app.display.iter().any(|&l| linha_contem(&app, l, "texto A1")));
            assert!(app.display.iter().any(|&l| linha_contem(&app, l, "texto A")));
        }

        #[test]
        fn z_abre_o_pai_e_o_filho_continua_dobrado() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "A1");
            dobrar(&mut app, "A");
            app.view.scroll = 0;
            app.on_key(ev(KeyCode::Char('z'))); // abre A
            assert!(app.display.iter().any(|&l| linha_contem(&app, l, "texto A")));
            assert!(!app.display.iter().any(|&l| linha_contem(&app, l, "texto A1")), "A1 segue dobrado");
        }

        #[test]
        fn maiusculo_dobra_tudo_se_alguma_esta_aberta_e_senao_abre_tudo() {
            let mut app = app_com_texto(DOC_FOLD);
            let total = app.total_lines();
            app.on_key(ev(KeyCode::Char('Z')));
            let dobrado = app.total_lines();
            assert!(dobrado < total);
            assert_eq!(app.folded.len(), 3, "A, A1 e B");
            app.on_key(ev(KeyCode::Char('Z')));
            assert_eq!(app.total_lines(), total);
            assert!(app.folded.is_empty());
        }

        #[test]
        fn maiusculo_com_uma_dobrada_dobra_o_resto() {
            let mut app = app_com_texto(DOC_FOLD);
            dobrar(&mut app, "B");
            app.on_key(ev(KeyCode::Char('Z')));
            assert_eq!(app.folded.len(), 3);
        }

        #[test]
        fn z_sem_titulos_avisa_sem_quebrar() {
            for md in ["só texto\n", ""] {
                let mut app = app_com_texto(md);
                app.on_key(ev(KeyCode::Char('z')));
                app.on_key(ev(KeyCode::Char('Z')));
                assert!(app.folded.is_empty());
                assert!(app.notice.as_deref().is_some_and(|n| n.contains("títulos")), "{:?}", app.notice);
            }
        }

        #[test]
        fn z_no_modo_fonte_e_em_json_avisa_sem_quebrar() {
            let mut app = app_com_texto("# A\n\nx\n");
            app.on_key(ev(KeyCode::Char('m')));
            app.rerender(60);
            app.on_key(ev(KeyCode::Char('z')));
            assert!(app.folded.is_empty());
            let mut json = test_app(Source::Stdin, r#"[{"a":1}]"#);
            json.doc.mode = cli::Mode::Json;
            json.rerender(60);
            json.view.height = 10;
            json.on_key(ev(KeyCode::Char('Z')));
            assert!(json.folded.is_empty());
        }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv z_dobra 2>&1 | tail -8)`
Expected: FAIL (`z` ainda não faz nada: o total de linhas não encolhe).

- [ ] **Step 3: Implementar**

Em `impl App`:

```rust
    /// `z`: dobra ou abre a seção do título mais fundo na linha de referência.
    fn toggle_fold(&mut self) {
        let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) else {
            self.notice = Some("nenhum título aqui para dobrar (documento sem títulos?)".to_string());
            return;
        };
        let key = fold::fold_keys(&self.rendered.outline)[idx].clone();
        if !self.folded.remove(&key) {
            self.folded.insert(key);
        }
        let line = self.rendered.outline[idx].line;
        self.recompute_display();
        self.scroll_to_line(line);
    }

    /// `Z`: dobra todas as seções se alguma está aberta; senão abre todas.
    fn toggle_fold_all(&mut self) {
        if self.rendered.outline.is_empty() {
            self.notice = Some("o documento não tem títulos para dobrar".to_string());
            return;
        }
        let keys = fold::fold_keys(&self.rendered.outline);
        let anchor = self.ref_line();
        if keys.iter().all(|k| self.folded.contains(k)) {
            self.folded.clear();
        } else {
            self.folded.extend(keys);
        }
        self.recompute_display();
        self.scroll_to_line(anchor);
    }
```

Em `on_key`, junto de `y`/`Y`:

```rust
            KeyCode::Char('z') => self.toggle_fold(),
            KeyCode::Char('Z') => self.toggle_fold_all(),
```

Barra de status (`status_line`): a string do lado direito passa a `"j/k rolar  [ ] título  / buscar  z dobrar  t tabela  y/Y copiar  m fonte  q sair "` (ajuste o teste da barra se comparar o texto).

- [ ] **Step 4: Documentação**

- `README.md`, lista de teclas: "`z` dobra ou abre a seção atual; `Z` dobra todas (ou abre todas). O título dobrado mostra `▸ N linhas`; a busca abre a dobra que esconde o resultado."
- `CLAUDE.md`, "Arquitetura": "- `src/fold.rs`: dobra de seções. `App.display` lista as linhas renderizadas visíveis e `Viewport.scroll` indexa `display`, não `rendered.lines`; use `ref_line`/`window_lines`/`scroll_to_line`/`pos_of` para converter. Chave da dobra = (título, ocorrência)."
- Spec, F7.3: "A integração com `<details>` fica fora desta entrega: `<details>` já aparece expandido e não tem título no `outline`. Tabela e bloco de código escondidos numa dobra não são alvo de `t`/`y`."

- [ ] **Step 5: Rodar tudo, clippy e commit**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error") && cargo clippy --all-targets 2>&1 | tail -5`
Expected: verde, sem avisos.

Validação manual (precisa de terminal): `cargo run --release -- README.md`, tecle `z`, `Z`, `]`, `/` por um termo dentro de uma seção dobrada.

```bash
git add src/main.rs README.md CLAUDE.md docs/superpowers
git commit -m "feat(tui): z dobra a seção e Z dobra ou abre todas (F7.3)"
```

---

## Self-Review

**Cobertura da spec:** estado `folded` com chave `(título, n-ésima ocorrência)` → Task 1 e 2. `display` recalculado em `rerender` e a cada dobra, rolagem/desenho/`]`/`[` sobre `display` → Task 2. Sufixo ` ▸ N linhas` → `screen_lines`. `z`/`Z` → Task 3. Busca abre a dobra → `reveal` em `goto_match`. Testes pedidos: `display` com seções aninhadas, dobrar título dentro de título dobrado, chave estável após reload, busca abrindo a dobra → Tasks 1–3. Integração com `<details>` (PLANO.md) fica fora, registrada na spec.

**Placeholders:** nenhum.

**Tipos:** `FoldKey = (String, usize)`, `fold_keys(&[OutlineEntry]) -> Vec<FoldKey>`, `Display { lines: Vec<usize>, hidden: HashMap<usize, usize> }` e os métodos de `App` têm a mesma assinatura em todas as tarefas; `scroll_to_line(usize)`, `ref_line() -> usize` e `window_lines() -> (usize, usize)` mantêm a assinatura criada na F7.4.

**Review Focus → testes:** filho dobrado com pai aberto (`abrir_o_pai_deixa_o_filho_dobrado`, `z_abre_o_pai_e_o_filho_continua_dobrado`); títulos repetidos (`dois_titulos_iguais_dobram_separados`, `chaves_numeram_as_ocorrencias_do_mesmo_titulo`); reload/largura/título sumido (`dobras_sobrevivem_a_reload_e_a_nova_largura`, `dobra_de_titulo_que_sumiu_nao_quebra`, `chave_de_titulo_que_sumiu_e_ignorada`); `t`/`y`/`]` com dobras (`t_nao_abre_tabela_escondida_numa_secao_dobrada`, `y_nao_copia_bloco_de_codigo_escondido`, `colchetes_pulam_so_entre_titulos_visiveis`); sem títulos/fonte/JSON (`z_sem_titulos_avisa_sem_quebrar`, `z_no_modo_fonte_e_em_json_avisa_sem_quebrar`).
