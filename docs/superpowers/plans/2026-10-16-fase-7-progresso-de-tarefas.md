# F7.2 — Progresso de task lists: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mostrar o andamento das tarefas (`- [x]`/`- [ ]`): contagem `feitas/total` do documento e da seção atual no rodapé da TUI, e ` [feitas/total]` ao lado dos títulos no `--toc`.

**Architecture:** O desenho dos itens já existe (`☑`/`☐`). A fundação da F7.4 já registra `Rendered.tasks: Vec<TaskItem>` e `section_range`. Este plano só **conta**: duas funções puras em `render.rs` (`count_tasks`, `section_tasks`), o `toc` passa a anexar a contagem, e `App::task_summary` alimenta a barra de status.

**Tech Stack:** Rust 2021. Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md` (seção "F7.2 Progresso de task lists").

**Pré-requisito:** o plano `docs/superpowers/plans/2026-10-16-fase-7-fundacao-e-copiar.md`, Tasks 1 e 3 executadas (`Rendered.tasks`, `TaskItem`, `section_range`, `section_at`, `App::ref_line`, helper de teste `app_com_texto`).

## Global Constraints

- Mensagens e docs em português, com acentuação correta.
- Sem mudança no desenho dos itens da lista.
- A contagem de um título inclui as subseções (usa `section_range`).
- Sem tarefas no documento, nada aparece no rodapé e o `--toc` fica idêntico ao atual.
- `--plain` não muda (a contagem só aparece na barra da TUI e no `--toc`).
- Regra do `CLAUDE.md`: nenhum teste aloca memória sem teto; rode `(ulimit -v 6000000; cargo test 2>&1 | tail -20)`.
- `cargo clippy --all-targets` sem avisos novos.

## Review Focus

- `- [x]` dentro de bloco de código e `[ ]` em texto corrido não são tarefas e não entram na conta (Task 1).
- Tarefa marcada dentro de citação e de lista aninhada entra na conta do documento e da seção (Task 1).
- Tarefas antes do primeiro título contam no documento mas em nenhuma seção; barra não pode entrar em pânico sem títulos (Task 2).
- Modo fonte (`m`), JSON e CSV: sem tarefas, sem contador, sem pânico (Task 2).
- Documento todo de uma seção só: o rodapé não repete a mesma contagem entre parênteses (Task 2).

---

## File Structure

- `src/render.rs`: `TaskCount`, `count_tasks`, `section_tasks`, `toc` com contagem.
- `src/main.rs`: `App::task_summary` e a linha na barra de status.
- `README.md`, `CLAUDE.md`: docs.

## Pontos do código atual que o plano usa

- Da F7.4: `TaskItem { line, done }`, `Rendered.tasks`, `section_range(&[OutlineEntry], idx, total) -> (usize, usize)`, `section_at(&[OutlineEntry], line) -> Option<usize>`, `App::ref_line()`.
- `pub fn toc(r: &Rendered) -> String` imprime cada título indentado por nível; usado por `--toc` em `run_cli`.
- `App::status_left()` monta `" nome  pct%…"`, depois a busca e, por último, `self.notice`.

---

### Task 1: Contagem em `render.rs` e `--toc`

**Files:**
- Modify: `src/render.rs`
- Test: `src/render.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `TaskItem`, `section_range`.
- Produces (usadas pela Task 2):
  - `#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)] pub struct TaskCount { pub done: usize, pub total: usize }`
  - `pub fn count_tasks(tasks: &[TaskItem], from: usize, to: usize) -> TaskCount` (linhas `[from, to)`)
  - `pub fn section_tasks(r: &Rendered, idx: usize) -> TaskCount`

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/render.rs`:

```rust
    fn todas(r: &Rendered) -> TaskCount {
        count_tasks(&r.tasks, 0, usize::MAX)
    }

    #[test]
    fn conta_tarefas_em_lista_aninhada() {
        let r = render("- [x] a\n- [ ] b\n  - [x] c\n", 40);
        assert_eq!(todas(&r), TaskCount { done: 2, total: 3 });
    }

    #[test]
    fn conta_tarefa_marcada_dentro_de_citacao() {
        let r = render("> - [x] q\n> - [ ] w\n", 40);
        assert_eq!(todas(&r), TaskCount { done: 1, total: 2 });
    }

    #[test]
    fn nao_conta_colchetes_em_codigo_nem_em_texto_corrido() {
        let r = render("```\n- [x] falso\n```\n\ntexto [ ] solto\n\n[x] sem lista\n", 40);
        assert_eq!(todas(&r), TaskCount::default());
    }

    #[test]
    fn count_tasks_usa_intervalo_fechado_aberto() {
        let tasks = [TaskItem { line: 2, done: true }, TaskItem { line: 5, done: false }];
        assert_eq!(count_tasks(&tasks, 2, 5), TaskCount { done: 1, total: 1 });
        assert_eq!(count_tasks(&tasks, 2, 6), TaskCount { done: 1, total: 2 });
        assert_eq!(count_tasks(&tasks, 3, 5), TaskCount::default());
    }

    #[test]
    fn secao_sem_tarefas_conta_zero() {
        let r = render("# A\n\n- [x] a\n\n# B\n\ntexto\n", 40);
        assert_eq!(section_tasks(&r, 0), TaskCount { done: 1, total: 1 });
        assert_eq!(section_tasks(&r, 1), TaskCount::default());
    }

    #[test]
    fn contagem_do_titulo_inclui_as_subsecoes() {
        let r = render("# A\n\n- [x] a\n\n## B\n\n- [ ] b\n\n## C\n\n- [x] c\n", 40);
        assert_eq!(section_tasks(&r, 0), TaskCount { done: 2, total: 3 });
        assert_eq!(section_tasks(&r, 1), TaskCount { done: 0, total: 1 });
        assert_eq!(section_tasks(&r, 2), TaskCount { done: 1, total: 1 });
    }

    #[test]
    fn tarefa_antes_do_primeiro_titulo_conta_so_no_documento() {
        let r = render("- [x] solta\n\n# A\n\ntexto\n", 40);
        assert_eq!(todas(&r), TaskCount { done: 1, total: 1 });
        assert_eq!(section_tasks(&r, 0), TaskCount::default());
    }

    #[test]
    fn toc_acrescenta_a_contagem_aos_titulos_com_tarefas() {
        let r = render("# A\n\n- [x] a\n\n## B\n\n- [ ] b\n\n# C\n\ntexto\n", 40);
        assert_eq!(toc(&r), "A [1/2]\n  B [0/1]\nC\n");
    }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv render::tests::conta 2>&1 | tail -8)`
Expected: erro de compilação (`TaskCount`, `count_tasks`, `section_tasks` não existem).

- [ ] **Step 3: Implementar**

Em `src/render.rs`, perto de `section_range`:

```rust
/// Tarefas feitas e total num trecho do documento.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct TaskCount {
    pub done: usize,
    pub total: usize,
}

/// Conta as tarefas cujas linhas estão em `[from, to)`.
pub fn count_tasks(tasks: &[TaskItem], from: usize, to: usize) -> TaskCount {
    let mut c = TaskCount::default();
    for t in tasks.iter().filter(|t| t.line >= from && t.line < to) {
        c.total += 1;
        if t.done {
            c.done += 1;
        }
    }
    c
}

/// Tarefas da seção `idx`, contando as subseções.
pub fn section_tasks(r: &Rendered, idx: usize) -> TaskCount {
    let (start, end) = section_range(&r.outline, idx, r.lines.len());
    count_tasks(&r.tasks, start, end)
}
```

E em `toc`, troque o corpo do laço por:

```rust
    for (i, e) in r.outline.iter().enumerate() {
        let _ = write!(out, "{}{}", "  ".repeat(usize::from(e.level.saturating_sub(1))), e.title);
        let c = section_tasks(r, i);
        if c.total > 0 {
            let _ = write!(out, " [{}/{}]", c.done, c.total);
        }
        out.push('\n');
    }
```

- [ ] **Step 4: Rodar e ver passar**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error")`
Expected: verde, inclusive `toc_indenta_por_nivel` e os demais testes antigos do `toc`.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -10`

```bash
git add src/render.rs
git commit -m "feat(render): contagem de tarefas por documento e seção, também no --toc (F7.2)"
```

---

### Task 2: Contador no rodapé da TUI e documentação

**Files:**
- Modify: `src/main.rs`, `README.md`, `CLAUDE.md`, `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`
- Test: `src/main.rs` (módulo `tests`, no submódulo que já tem `app_com_texto` e `ev`)

**Interfaces:**
- Consumes: `render::count_tasks`, `render::section_tasks`, `render::section_at`, `App::ref_line()`, `App::status_left()`.
- Produces: `App::task_summary(&self) -> Option<String>`.

- [ ] **Step 1: Escrever os testes que falham**

```rust
        #[test]
        fn rodape_mostra_o_andamento_das_tarefas_do_documento() {
            let app = app_com_texto("- [x] a\n- [ ] b\n- [ ] c\n");
            assert!(app.status_left().contains("☑ 1/3"), "{}", app.status_left());
        }

        #[test]
        fn rodape_sem_tarefas_nao_mostra_contador() {
            let app = app_com_texto("# A\n\ntexto\n");
            assert!(!app.status_left().contains('☑'), "{}", app.status_left());
        }

        #[test]
        fn rodape_acrescenta_a_secao_atual_quando_ela_tem_tarefas() {
            let mut app = app_com_texto("# A\n\n- [x] a\n\n# B\n\n- [ ] b\n- [ ] c\n");
            app.view.scroll = app.rendered.outline[1].line;
            let s = app.status_left();
            assert!(s.contains("☑ 1/3") && s.contains("(seção 0/2)"), "{s}");
        }

        #[test]
        fn rodape_nao_repete_a_contagem_quando_a_secao_e_o_documento() {
            let app = app_com_texto("# A\n\n- [x] a\n- [ ] b\n");
            let s = app.status_left();
            assert!(s.contains("☑ 1/2") && !s.contains("seção"), "{s}");
        }

        #[test]
        fn rodape_com_tarefa_antes_do_primeiro_titulo_nao_quebra() {
            let app = app_com_texto("- [x] solta\n\n# A\n\ntexto\n");
            assert!(app.status_left().contains("☑ 1/1"));
        }

        #[test]
        fn rodape_no_modo_fonte_nao_mostra_contador() {
            let mut app = app_com_texto("- [x] a\n");
            app.on_key(ev(KeyCode::Char('m')));
            app.rerender(60);
            assert!(!app.status_left().contains('☑'), "{}", app.status_left());
        }
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `(ulimit -v 6000000; cargo test --bin mdv rodape_ 2>&1 | tail -10)`
Expected: FAIL (`rodape_mostra_o_andamento...`: a barra ainda não tem o contador). Os outros dois sem contador passam já agora; é esperado.

- [ ] **Step 3: Implementar**

Em `impl App` (perto de `status_left`):

```rust
    /// `☑ feitas/total` do documento e, se a seção atual tem tarefas e difere do todo, `(seção f/t)`.
    fn task_summary(&self) -> Option<String> {
        let all = render::count_tasks(&self.rendered.tasks, 0, usize::MAX);
        if all.total == 0 {
            return None;
        }
        let mut s = format!("☑ {}/{}", all.done, all.total);
        if let Some(idx) = render::section_at(&self.rendered.outline, self.ref_line()) {
            let c = render::section_tasks(&self.rendered, idx);
            if c.total > 0 && c != all {
                let _ = write!(s, " (seção {}/{})", c.done, c.total);
            }
        }
        Some(s)
    }
```

Em `status_left`, imediatamente **antes** de `if let Some(n) = &self.notice {`:

```rust
        if let Some(t) = self.task_summary() {
            let _ = write!(s, "  {t}");
        }
```

- [ ] **Step 4: Documentação**

- `README.md`: na descrição do `--toc` (ou da lista de recursos), acrescente: "Listas de tarefas (`- [x]`/`- [ ]`) mostram o andamento: o rodapé da leitura traz `☑ feitas/total` do documento e da seção atual, e o `--toc` acrescenta `[feitas/total]` aos títulos."
- `CLAUDE.md`, item de `src/render.rs`: acrescente "`count_tasks`/`section_tasks` contam as tarefas (`Rendered.tasks`) por trecho e por seção."
- Spec, seção F7.2: "O rodapé omite `(seção …)` quando a seção tem a mesma contagem do documento. Tarefas antes do primeiro título contam só no documento."

- [ ] **Step 5: Rodar tudo, clippy e commit**

Run: `(ulimit -v 6000000; cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error") && cargo clippy --all-targets 2>&1 | tail -5`
Expected: verde, sem avisos.

Validação manual: `cargo run --release -- --toc docs/superpowers/plans/2026-10-16-fase-7-progresso-de-tarefas.md` mostra os títulos com `[0/N]`-style (este plano tem checkboxes `- [ ]` em listas).

```bash
git add src/main.rs README.md CLAUDE.md docs/superpowers
git commit -m "feat(tui): andamento das tarefas no rodapé e no --toc (F7.2)"
```

---

## Self-Review

**Cobertura da spec:** `tasks` vem da fundação (um `TaskItem` por marcador) → pré-requisito. Rodapé com `☑ feitas/total` do documento e da seção atual quando há tarefas nela; sem tarefas, nada → Task 2. `--toc` com ` [feitas/total]` incluindo subseções → Task 1. Sem mudança no desenho dos itens → nada tocado. Testes pedidos: lista aninhada, item dentro de citação, seção sem tarefas, formato do `--toc` → Task 1.

**Placeholders:** nenhum.

**Tipos:** `TaskCount`, `count_tasks(&[TaskItem], usize, usize)`, `section_tasks(&Rendered, usize)` iguais nas duas tarefas; `section_at` e `ref_line` vêm da F7.4.

**Review Focus → testes:** código e texto solto (`nao_conta_colchetes_em_codigo_nem_em_texto_corrido`); citação e aninhada (`conta_tarefa_marcada_dentro_de_citacao`, `conta_tarefas_em_lista_aninhada`); antes do primeiro título (`tarefa_antes_do_primeiro_titulo_conta_so_no_documento`, `rodape_com_tarefa_antes_do_primeiro_titulo_nao_quebra`); fonte/JSON (`rodape_no_modo_fonte_nao_mostra_contador`; JSON e CSV nunca preenchem `Rendered.tasks`); repetição (`rodape_nao_repete_a_contagem_quando_a_secao_e_o_documento`).
