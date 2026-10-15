# F7.7 + F7.8 — Quebra de linha em células e zoom de tabela: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** (F7.7) Parar de cortar células largas de tabela com `…` e quebrá-las em várias linhas, no Markdown e no CSV/TSV. (F7.8) Abrir uma tabela numa tela própria, na largura inteira do terminal, com cabeçalho e primeira coluna fixos e rolagem por colunas (`t`, `--table N`).

**Architecture:** `src/render.rs` troca o corte de `fit()` por `wrap_cell()` (reaproveita `wrap`, o quebrador de palavras do parágrafo) e `draw_table_row` passa a devolver várias linhas. O desenho da tabela sai de `Renderer::render_table` para a função livre `layout_table`, que recebe a largura e a primeira coluna rolável; o `Renderer` guarda cada tabela desenhada em `Rendered.tables` (`TableSpan` com um `TableData` que sabe se redesenhar em outra largura). `src/main.rs` ganha o estado `Zoom`, que chama `TableData::draw` com a largura do terminal. Rolar "na horizontal" é rolar por **coluna** (`first_col`), não por caractere: o `Viewport` continua só vertical.

**Tech Stack:** Rust 2021, `ratatui`, `pulldown-cmark::Alignment`, `unicode-width`. Nenhuma dependência nova.

**Spec:** `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`, seções "F7.7 Quebra de linha em células de tabela" e "F7.8 Zoom de tabela"; itens F7.7 e F7.8 do `PLANO.md`. Este plano refina três pontos da spec (ver "Decisões que refinam a spec").

## Global Constraints

- Mensagens de erro, ajuda e README em português, com acentuação correta.
- Esta fase não acrescenta I/O: sem rede, sem subprocesso, sem dependência nova.
- Todo texto de célula que chega ao terminal já passa por `render::sanitize` (Markdown na entrada do `pulldown-cmark`, CSV em `render_table_only`). Não escreva `Span` com texto cru novo.
- Saída de `--plain` nunca contém `\x1b` além do SGR gerado pelo `mdv`.
- Teto de **8 linhas por célula** (`MAX_CELL_LINES`); `…` na última linha ao estourar.
- Cada linha de tabela (borda, dado, rodapé) tem no máximo a largura pedida: `assert_fits` continua verdadeiro.
- A renderização de uma tabela com célula de 3 000 000 de caracteres continua abaixo de 5 s (teste `tabela_larga_renderiza_em_tempo_limitado` já existe e tem de continuar passando).
- `cargo clippy` sem avisos novos; `cargo test` verde ao fim de cada tarefa.

## Review Focus

- Célula só com URL/hash maior que a coluna (sem espaço): quebra em caractere, sem perder nenhum caractere até o teto. Teste na Task 1.
- Célula com 3 milhões de caracteres: o corte antes de quebrar evita alocar milhões de linhas. Teste existente + `celula_enorme_respeita_o_teto` na Task 1.
- Célula vazia e célula com caractere largo (CJK, emoji) em coluna estreita: sem pânico, linha com a mesma largura das outras. Teste na Task 1.
- Tabela dentro de lista ou citação: o intervalo `start..end` em `Rendered.tables` cobre as linhas com recuo, e o zoom ignora o recuo. Teste na Task 2.
- Tabela com uma só coluna, e tabela com mais colunas do que cabem na largura (`… +N colunas`): `first_col` nunca sai do intervalo válido. Testes na Task 2.
- Reload com o zoom aberto e a tabela sumindo do arquivo: o zoom fecha em vez de panicar por índice. Teste na Task 3.
- `t` sem tabela na tela, `--table 0`, `--table 99` com 2 tabelas: aviso na barra de status ou erro de uso, nunca pânico. Testes nas Tasks 3 e 4.
- Terminal muito estreito (< 20 colunas) no zoom: `clamp_width` segura. Coberto por `clamp_width` e pelo teste de `draw` com largura 20.

---

## File Structure

| Arquivo | Responsabilidade nesta fase |
|---|---|
| `src/render.rs` | `wrap_cell`/`clip_pieces`, `draw_table_row` multilinha, `layout_table`, `TableData`, `TableSpan`, `TableView`, `Rendered.tables` |
| `src/main.rs` | `Zoom`, `App::open_zoom`/`open_visible_table`/`refresh_zoom`/`on_zoom_key`/`draw_zoom`, aviso (`notice`), tecla `t`, ajuda |
| `src/cli.rs` | flag `--table <N>` |
| `tests/cli.rs` | CSV com célula longa em `-p`, `--table` inválido |
| `samples/tabelas.md` | seção de célula longa |
| `README.md`, `PLANO.md`, spec | documentação e checkboxes |

`src/data.rs` **não muda**: `render_table_data` chama `render_table_only`, que passa a ter o wrap e a registrar a tabela em `Rendered.tables`; o rodapé `N de M linhas` é anexado depois, fora de `start..end`, e por isso não aparece no zoom.

## Pontos do código atual que o plano usa

- `src/render.rs`: `Piece` (privado: `Text`, `Code`, `Break`), `fit(cell, w)`, `wrap(&[Piece], avail) -> Vec<Vec<Span>>` (quebra em palavra e, para palavra maior que a linha, em caractere via `split_by_width`), `spans_width`, `natural_widths(rows, ncols)`, `compute_column_widths(natural, avail)`, `table_border`, `draw_table_row`, `Renderer::render_table(&TableBuilder)` (chamado em `TagEnd::Table` e em `render_table_only`), `Renderer::prefixes() -> (first, cont)`, `Renderer::take_marker()`.
- Constantes `CELL_OVERHEAD = 3` e `MIN_COL_WIDTH = 3`.
- Testes de `render.rs`: `rendered_text_at(md, width) -> Vec<String>`, `assert_fits(md, width)`, `render(md, width)`.
- `src/main.rs`: `Viewport { scroll, height }` com `max_scroll`, `scroll_by`, `scroll_to`; `App::on_key`, `App::draw`, `App::rerender`, `App::status_left`, `App::status_line`; helper de teste `test_app(source, md)`; constante `HELP`.
- `src/cli.rs`: `Cli`, `parse`, helper de teste `run(&[..])`.
- Há alterações locais não commitadas em `src/main.rs` (rótulo `[ ] pular título` na barra e o teste dele). Não as descarte; os testes novos convivem com elas.

## Decisões que refinam a spec

1. **Rolagem horizontal é por coluna, não por caractere.** Com o wrap, qualquer tabela cabe em largura `MIN_COL_WIDTH` por coluna; o que sobra é tabela com *colunas demais* (`… +N colunas`). Rolar coluna a coluna resolve isso sem mexer no `Viewport`. A primeira coluna fica fixa (é a que identifica a linha) e o cabeçalho fica fixo na vertical. Teclas: `h`/`l` (e ←/→), `0` primeira, `$` última.
2. **CSV/TSV e JSON em tabela usam o mesmo `t`.** Não há "modo tela cheia sem rodapé" à parte: o zoom mostra a tabela sem o rodapé de qualquer jeito, e `t` acha a tabela na tela como no Markdown.
3. **`--table N` fora do intervalo não é erro de uso**, pois só sabemos quantas tabelas existem depois de renderizar: mostra o aviso `tabela N não existe (há M)` na barra e abre o documento normal. `--table 0` ou não numérico é erro de uso (código 2).

---

### Task 1: Quebra de linha em células (F7.7)

**Files:**
- Modify: `src/render.rs` (`fit` fica; novas `clip_pieces`, `wrap_cell`; `draw_table_row` e `render_table`)
- Test: `src/render.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `wrap(&[Piece], usize) -> Vec<Vec<Span<'static>>>`, `fit(&[Piece], usize) -> (Vec<Span<'static>>, usize)`, `spans_width(&[Span<'_>]) -> usize`.
- Produces: `const MAX_CELL_LINES: usize = 8;`, `fn clip_pieces(cell: &[Piece], budget: usize) -> (Vec<Piece>, bool)`, `fn wrap_cell(cell: &[Piece], w: usize) -> Vec<(Vec<Span<'static>>, usize)>`, e `fn draw_table_row(prefix: &[Span<'static>], row: &[Vec<Piece>], widths: &[usize], aligns: &[Alignment]) -> Vec<Line<'static>>` (a assinatura de entrada não muda nesta tarefa; a Task 2 troca `row` por `&[&[Piece]]`).

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/render.rs`, depois de `tabela_com_colunas_ocultas_avisa`, acrescente:

```rust
    const FRASE_LONGA: &str = "uma descricao bem longa que precisa quebrar em varias linhas dentro da celula";

    fn tabela_de_uma_celula(texto: &str, alinhamento: &str) -> String {
        format!("| titulo |\n|{alinhamento}|\n| {texto} |\n")
    }

    #[test]
    fn celula_longa_quebra_em_varias_linhas() {
        let md = format!("| nome | descricao |\n|---|---|\n| a | {FRASE_LONGA} |\n");
        let lines = rendered_text_at(&md, 40);
        let junto = lines.join("\n");
        assert!(!junto.contains('…'), "não deve truncar: {junto}");
        for palavra in FRASE_LONGA.split(' ') {
            assert!(junto.contains(palavra), "faltou {palavra:?}: {junto}");
        }
        // borda, cabeçalho, separador, 3+ linhas de dado, borda
        assert!(lines.len() >= 8, "{lines:?}");
        assert_fits(&md, 40);
    }

    #[test]
    fn palavra_maior_que_a_coluna_quebra_por_caractere() {
        let md = tabela_de_uma_celula(&"x".repeat(100), "---");
        let junto = rendered_text_at(&md, 30).join("");
        assert_eq!(junto.matches('x').count(), 100, "{junto}");
        assert_fits(&md, 30);
    }

    #[test]
    fn teto_de_linhas_termina_com_reticencias() {
        let md = tabela_de_uma_celula(&"palavra ".repeat(200), "---");
        let lines = rendered_text_at(&md, 30);
        let dados: Vec<&String> = lines.iter().filter(|l| l.contains("palavra") || l.contains('…')).collect();
        assert_eq!(dados.len(), MAX_CELL_LINES, "{lines:?}");
        assert!(dados.last().unwrap().contains('…'), "{dados:?}");
        assert!(dados[..MAX_CELL_LINES - 1].iter().all(|l| !l.contains('…')), "{dados:?}");
    }

    #[test]
    fn celula_enorme_respeita_o_teto() {
        let md = tabela_de_uma_celula(&"x".repeat(500), "---");
        let lines = rendered_text_at(&md, 30);
        let dados: Vec<&String> = lines.iter().filter(|l| l.contains('x')).collect();
        assert_eq!(dados.len(), MAX_CELL_LINES, "{lines:?}");
        assert!(dados.last().unwrap().contains('…'), "{dados:?}");
    }

    #[test]
    fn alinhamento_a_direita_vale_em_cada_linha_da_celula() {
        let md = tabela_de_uma_celula("1234567890 1234567890", "--:");
        let lines = rendered_text_at(&md, 20);
        let dados: Vec<&String> = lines.iter().filter(|l| l.contains("1234567890")).collect();
        assert_eq!(dados.len(), 2, "{lines:?}");
        assert!(dados.iter().all(|l| l.ends_with("1234567890 │")), "{dados:?}");
    }

    #[test]
    fn todas_as_linhas_da_tabela_tem_a_mesma_largura() {
        let md = format!(
            "| a | b | c |\n|:--|:-:|--:|\n| {FRASE_LONGA} | 世界世界世界世界 | |\n| x | y | z |\n"
        );
        let larguras: std::collections::HashSet<usize> =
            render(&md, 40).lines.iter().filter(|l| !l.spans.is_empty()).map(Line::width).collect();
        assert_eq!(larguras.len(), 1, "{larguras:?}");
    }

    #[test]
    fn celula_com_estilo_mantem_o_estilo_apos_quebrar() {
        let md = tabela_de_uma_celula("**negrito comprido que quebra** fim", "---");
        let out = render(&md, 20);
        let negrito = out
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.style.add_modifier.contains(Modifier::BOLD) && s.content.contains("negrito"))
            .count();
        assert!(negrito >= 1);
    }

    #[test]
    fn tabela_so_de_dados_quebra_celula_longa() {
        let rows = vec![vec!["id".to_string(), FRASE_LONGA.to_string()]];
        let out = render_table_only(rows, false, vec![Alignment::Left; 2], 30);
        let junto: String = out.lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(junto.contains("celula") && !junto.contains('…'), "{junto}");
    }
```

Se `line_to_string` ainda não existir no módulo de testes, acrescente junto dos helpers (perto de `rendered_text_at`):

```rust
    fn line_to_string(l: &Line<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }
```

- [ ] **Step 2: Rodar os testes e ver falhar**

Run: `cargo test --lib render::tests::celula render::tests::palavra_maior render::tests::teto_de render::tests::alinhamento_a_direita render::tests::todas_as_linhas render::tests::tabela_so_de_dados 2>&1 | tail -30`
Expected: erro de compilação (`MAX_CELL_LINES` não definido). Depois de definir a constante vazia, os testes falham por truncar com `…`.

- [ ] **Step 3: Implementar `clip_pieces`, `wrap_cell` e a linha multilinha**

Em `src/render.rs`, logo depois de `fit` (≈ linha 1175), acrescente:

```rust
/// Teto de linhas por célula de tabela; o que passa disso termina em "…".
const MAX_CELL_LINES: usize = 8;

/// Mesma natureza de `piece` (texto ou código) com outro conteúdo; `Break` vira espaço.
fn with_text(piece: &Piece, text: String) -> Piece {
    match piece {
        Piece::Code(_, st) => Piece::Code(text, *st),
        Piece::Text(_, st) => Piece::Text(text, *st),
        Piece::Break => Piece::Text(text, Style::default()),
    }
}

/// Mantém só os primeiros `budget` colunas da célula; o `bool` diz se algo foi cortado.
/// Evita quebrar em linhas um texto de milhões de caracteres que só mostraria `MAX_CELL_LINES`.
fn clip_pieces(cell: &[Piece], budget: usize) -> (Vec<Piece>, bool) {
    let mut used = 0;
    let mut out = Vec::new();
    for piece in cell {
        let text = piece.as_text().0;
        let room = budget - used;
        let w = text.width();
        if w <= room {
            out.push(with_text(piece, text.to_string()));
            used += w;
            continue;
        }
        let mut buf = String::new();
        let mut taken = 0;
        for ch in text.chars() {
            let cw = ch.width().unwrap_or(0);
            if taken + cw > room {
                break;
            }
            buf.push(ch);
            taken += cw;
        }
        if !buf.is_empty() {
            out.push(with_text(piece, buf));
        }
        return (out, true);
    }
    (out, false)
}

/// Quebra a célula em linhas de até `w` colunas (no máximo `MAX_CELL_LINES`), cada uma com a
/// sua largura. Se sobrar texto, a última linha termina em "…".
fn wrap_cell(cell: &[Piece], w: usize) -> Vec<(Vec<Span<'static>>, usize)> {
    let w = w.max(1);
    let (pieces, clipped) = clip_pieces(cell, w.saturating_mul(MAX_CELL_LINES));
    let mut lines = wrap(&pieces, w);
    let overflow = lines.len() > MAX_CELL_LINES;
    lines.truncate(MAX_CELL_LINES);
    if overflow || clipped {
        let last = lines.pop().unwrap_or_default();
        let style = last.last().map_or_else(Style::default, |s| s.style);
        let mut ps: Vec<Piece> =
            last.iter().map(|s| Piece::Text(s.content.to_string(), s.style)).collect();
        ps.push(Piece::Text("…".to_string(), style));
        lines.push(fit(&ps, w).0);
    }
    lines
        .into_iter()
        .map(|spans| {
            let n = spans_width(&spans);
            (spans, n)
        })
        .collect()
}
```

Troque `draw_table_row` (≈ linha 1072) por:

```rust
/// Linhas de dados da tabela: cada célula é quebrada na largura da sua coluna e alinhada;
/// a linha da tabela tem a altura da maior célula.
fn draw_table_row(
    prefix: &[Span<'static>],
    row: &[Vec<Piece>],
    widths: &[usize],
    aligns: &[Alignment],
) -> Vec<Line<'static>> {
    let dim = Style::default().fg(DIM);
    let cells: Vec<Vec<(Vec<Span<'static>>, usize)>> = widths
        .iter()
        .enumerate()
        .map(|(c, &width)| wrap_cell(row.get(c).map_or(&[][..], Vec::as_slice), width))
        .collect();
    let height = cells.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|li| {
            let mut v = prefix.to_vec();
            v.push(Span::styled("│", dim));
            for (c, &width) in widths.iter().enumerate() {
                let (spans, w) = cells[c].get(li).cloned().unwrap_or_default();
                let pad = width.saturating_sub(w);
                let (lead, trail) = match aligns.get(c).copied().unwrap_or(Alignment::None) {
                    Alignment::Right => (pad, 0),
                    Alignment::Center => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                v.push(Span::raw(format!(" {}", " ".repeat(lead))));
                v.extend(spans);
                v.push(Span::raw(format!("{} ", " ".repeat(trail))));
                v.push(Span::styled("│", dim));
            }
            Line::from(v)
        })
        .collect()
}
```

Em `Renderer::render_table`, troque o laço (≈ linhas 1025–1031) por:

```rust
        for (ri, row) in t.rows.iter().enumerate() {
            out.extend(draw_table_row(&cont, row, &widths, &t.aligns));
            if ri == 0 && t.has_header {
                out.push(table_border(&cont, &widths, ["├", "┼", "┤"]));
            }
        }
```

(`out.push(draw_table_row(...))` vira `out.extend(...)`.) Atualize também o comentário de `draw_table_row`/docs que ainda digam "truncada".

- [ ] **Step 4: Rodar os testes e ver passar**

Run: `cargo test --lib render 2>&1 | tail -15`
Expected: todos passam, inclusive `tabela_larga_renderiza_em_tempo_limitado` (célula de 3 milhões de caracteres). Se esse teste ficar lento, confira se `clip_pieces` é chamado antes de `wrap`.

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15` — Expected: sem avisos novos.

```bash
git add src/render.rs
git commit -m "feat(render): quebra células largas de tabela em várias linhas (F7.7)"
```

---

### Task 2: Tabela redesenhável (`TableData`, `layout_table`, `Rendered.tables`)

**Files:**
- Modify: `src/render.rs`
- Test: `src/render.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `draw_table_row`, `table_border`, `natural_widths`, `compute_column_widths`, `fit`, `Renderer::prefixes`, `Renderer::take_marker` (todas já existem).
- Produces (usadas pela Task 3):
  - `pub struct TableData` (campos privados `rows`, `aligns`, `has_header`), `#[derive(Clone)]`, com `pub fn total_cols(&self) -> usize` e `pub fn draw(&self, width: usize, first_col: usize) -> TableView`.
  - `pub struct TableSpan { pub start: usize, pub end: usize, pub data: TableData }` — `start..end` são índices em `Rendered.lines`.
  - `pub struct TableView { pub lines: Vec<Line<'static>>, pub pinned: usize, pub first_col: usize, pub shown_cols: usize, pub total_cols: usize }`.
  - `Rendered.tables: Vec<TableSpan>` (campo novo, `pub`).
  - `natural_widths(rows: &[Vec<Vec<Piece>>], idx: &[usize]) -> Vec<usize>` (troca `ncols` por lista de índices).
  - `render_table(&mut self, t: TableBuilder)` passa a receber por valor.

**Semântica de `first_col`:** a coluna 0 está sempre à vista. `first_col` é o índice (0-based) da primeira coluna **rolável** mostrada; `0` é aceito como "do início" e vira `1`. `first_col` é limitado para a última coluna ficar visível no máximo. Colunas mostradas: `[0] ++ first_col..first_col + (k-1)`, com `k = min(total, (largura-1)/(CELL_OVERHEAD+MIN_COL_WIDTH))`.

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/render.rs`:

```rust
    fn tabela_de_n_colunas(n: usize) -> String {
        let header: String = (1..=n).map(|i| format!(" c{i} |")).collect();
        let sep = "---|".repeat(n);
        let row: String = (1..=n).map(|i| format!(" v{i} |")).collect();
        format!("|{header}\n|{sep}\n|{row}\n")
    }

    #[test]
    fn render_registra_o_intervalo_de_cada_tabela() {
        let md = "antes\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\ndepois\n";
        let out = render(md, 40);
        assert_eq!(out.tables.len(), 1);
        let t = &out.tables[0];
        assert!(line_to_string(&out.lines[t.start]).starts_with('┌'));
        assert!(line_to_string(&out.lines[t.end - 1]).starts_with('└'));
        assert_eq!(t.data.total_cols(), 2);
    }

    #[test]
    fn tabela_em_citacao_e_lista_registra_o_intervalo_com_recuo() {
        let out = render("> | x |\n> |---|\n> | 1 |\n", 40);
        assert_eq!(out.tables.len(), 1);
        let t = &out.tables[0];
        assert!(line_to_string(&out.lines[t.start]).contains('┌'));
        let out = render("- item\n\n  | x |\n  |---|\n  | 1 |\n", 40);
        assert_eq!(out.tables.len(), 1);
        assert!(out.tables[0].end <= out.lines.len());
    }

    #[test]
    fn duas_tabelas_ficam_em_ordem_e_sem_sobreposicao() {
        let md = "| a |\n|---|\n| 1 |\n\ntexto\n\n| b |\n|---|\n| 2 |\n";
        let out = render(md, 40);
        assert_eq!(out.tables.len(), 2);
        assert!(out.tables[0].end <= out.tables[1].start);
    }

    #[test]
    fn render_table_only_registra_a_tabela() {
        let out = render_table_only(vec![vec!["a".into(), "b".into()]], false, vec![Alignment::Left; 2], 40);
        assert_eq!(out.tables.len(), 1);
        assert_eq!((out.tables[0].start, out.tables[0].end), (0, out.lines.len()));
    }

    #[test]
    fn draw_nao_tem_recuo_e_fixa_cabecalho() {
        let out = render("> | x | y |\n> |---|---|\n> | 1 | 2 |\n", 40);
        let v = out.tables[0].data.draw(40, 0);
        assert!(line_to_string(&v.lines[0]).starts_with('┌'), "sem o '▎ ' da citação");
        assert_eq!(v.pinned, 3, "borda, cabeçalho e separador");
        assert_eq!((v.first_col, v.shown_cols, v.total_cols), (1, 2, 2));
    }

    #[test]
    fn draw_sem_cabecalho_fixa_so_a_borda() {
        let out = render_table_only(vec![vec!["a".into()]], false, vec![Alignment::Left], 40);
        assert_eq!(out.tables[0].data.draw(40, 0).pinned, 1);
    }

    #[test]
    fn draw_com_uma_coluna_nao_estoura_o_indice() {
        let out = render("| só |\n|---|\n| 1 |\n", 40);
        let v = out.tables[0].data.draw(20, usize::MAX);
        assert_eq!((v.first_col, v.shown_cols, v.total_cols), (0, 1, 1));
    }

    #[test]
    fn draw_rola_por_coluna_e_mantem_a_primeira() {
        let out = render(&tabela_de_n_colunas(12), 20);
        let data = &out.tables[0].data;

        let v = data.draw(20, 0);
        assert_eq!((v.first_col, v.shown_cols), (1, 3));
        let cab = line_to_string(&v.lines[1]);
        assert!(cab.contains("│ c1 ") && cab.contains("c2"), "{cab}");
        assert!(line_to_string(v.lines.last().unwrap()).contains("+9 colunas"));

        let v = data.draw(20, 5);
        assert_eq!(v.first_col, 5);
        let cab = line_to_string(&v.lines[1]);
        assert!(cab.contains("│ c1 ") && cab.contains("c6") && !cab.contains("c2 "), "{cab}");

        let v = data.draw(20, usize::MAX);
        assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols, "última coluna à vista");
        assert!(line_to_string(&v.lines[1]).contains("c12"));
    }

    #[test]
    fn draw_cabe_em_qualquer_largura_pedida() {
        let out = render(&tabela_de_n_colunas(30), 80);
        for w in [20, 33, 80, 200] {
            for first in [0, 3, usize::MAX] {
                let v = out.tables[0].data.draw(w, first);
                assert!(v.lines.iter().all(|l| l.width() <= w), "largura {w}, coluna {first}");
            }
        }
    }
```

- [ ] **Step 2: Rodar os testes e ver falhar**

Run: `cargo test --lib render::tests 2>&1 | tail -15`
Expected: erro de compilação (`tables`, `draw`, `TableView` não existem).

- [ ] **Step 3: Implementar**

3a. Em `Rendered` (≈ linha 78) acrescente o campo:

```rust
    /// Tabelas desenhadas, em ordem, com o intervalo de linhas de cada uma (zoom `t`).
    pub tables: Vec<TableSpan>,
```

3b. Depois de `OutlineEntry`, acrescente os tipos públicos:

```rust
/// Tabela guardada para ser desenhada de novo em outra largura (zoom).
#[derive(Clone)]
pub struct TableData {
    rows: Vec<Vec<Vec<Piece>>>,
    aligns: Vec<Alignment>,
    has_header: bool,
}

/// Onde uma tabela está em `Rendered.lines` (`start..end`) e como redesenhá-la.
pub struct TableSpan {
    pub start: usize,
    pub end: usize,
    pub data: TableData,
}

/// Tabela desenhada numa largura: as linhas, quantas ficam fixas no topo e quais colunas aparecem.
pub struct TableView {
    pub lines: Vec<Line<'static>>,
    /// Linhas fixas no topo (borda, cabeçalho e separador; só a borda sem cabeçalho).
    pub pinned: usize,
    /// Índice (0-based) da primeira coluna rolável mostrada, já limitado ao intervalo válido.
    pub first_col: usize,
    /// Colunas desenhadas, contando a primeira (fixa).
    pub shown_cols: usize,
    pub total_cols: usize,
}

impl TableData {
    pub fn total_cols(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Desenha a tabela em `width` colunas, sem recuo. A coluna 0 fica sempre;
    /// as demais começam em `first_col`.
    pub fn draw(&self, width: usize, first_col: usize) -> TableView {
        layout_table(self, &[], &[], width, first_col)
    }
}
```

3c. Troque `natural_widths` por:

```rust
/// Maior largura de conteúdo de cada coluna de `idx` (mínimo 1).
fn natural_widths(rows: &[Vec<Vec<Piece>>], idx: &[usize]) -> Vec<usize> {
    let mut natural = vec![1usize; idx.len()];
    for row in rows {
        for (n, &c) in natural.iter_mut().zip(idx) {
            if let Some(cell) = row.get(c) {
                *n = (*n).max(cell.iter().map(Piece::width).sum());
            }
        }
    }
    natural
}
```

3d. Troque `draw_table_row` para receber as células já escolhidas (`row: &[&[Piece]]`) — só a linha `.map(|(c, &width)| wrap_cell(row.get(c).map_or(&[][..], Vec::as_slice), width))` muda para:

```rust
        .map(|(c, &width)| wrap_cell(row.get(c).copied().unwrap_or(&[]), width))
```

e o parâmetro vira `row: &[&[Piece]]`.

3e. Substitua `Renderer::render_table` inteiro por uma versão que delega a `layout_table`, e acrescente `layout_table` como função livre:

```rust
    fn render_table(&mut self, t: TableBuilder) {
        let (first, cont) = self.prefixes();
        let avail = self.width.saturating_sub(spans_width(&cont));
        let data = TableData { rows: t.rows, aligns: t.aligns, has_header: t.has_header };
        if data.total_cols() == 0 {
            return;
        }
        self.take_marker();
        let view = layout_table(&data, &first, &cont, avail, 0);
        let start = self.out.lines.len();
        let end = start + view.lines.len();
        self.out.lines.extend(view.lines);
        self.out.tables.push(TableSpan { start, end, data });
    }
```

```rust
/// Desenha a tabela em `avail` colunas. A coluna 0 fica sempre; as colunas roláveis mostradas
/// começam em `first_col` (limitado para a última coluna caber no máximo). Colunas que não
/// cabem (cada uma precisa de `MIN_COL_WIDTH` mais a borda) ficam de fora e são contadas no aviso.
fn layout_table(
    t: &TableData,
    first: &[Span<'static>],
    cont: &[Span<'static>],
    avail: usize,
    first_col: usize,
) -> TableView {
    let total = t.total_cols();
    let fit_cols = (avail.saturating_sub(1) / (CELL_OVERHEAD + MIN_COL_WIDTH)).max(1);
    let shown = total.min(fit_cols);
    let scrollable = shown.saturating_sub(1);
    let first_col = if total <= shown {
        usize::from(total > 1)
    } else {
        first_col.clamp(1, total - scrollable)
    };
    let idx: Vec<usize> = std::iter::once(0).chain(first_col..first_col + scrollable).collect();
    let aligns: Vec<Alignment> =
        idx.iter().map(|&c| t.aligns.get(c).copied().unwrap_or(Alignment::None)).collect();
    let widths = compute_column_widths(&natural_widths(&t.rows, &idx), avail);

    let mut lines: Vec<Line<'static>> = vec![table_border(first, &widths, ["┌", "┬", "┐"])];
    let mut pinned = 1;
    for (ri, row) in t.rows.iter().enumerate() {
        let cells: Vec<&[Piece]> =
            idx.iter().map(|&c| row.get(c).map_or(&[][..], Vec::as_slice)).collect();
        let drawn = draw_table_row(cont, &cells, &widths, &aligns);
        let is_head = ri == 0 && t.has_header;
        if is_head {
            pinned += drawn.len() + 1;
        }
        lines.extend(drawn);
        if is_head {
            lines.push(table_border(cont, &widths, ["├", "┼", "┤"]));
        }
    }
    lines.push(table_border(cont, &widths, ["└", "┴", "┘"]));
    let hidden = total - shown;
    if hidden > 0 {
        let note = [Piece::Text(format!("… +{hidden} colunas"), Style::default().fg(DIM))];
        let mut v = cont.to_vec();
        v.extend(fit(&note, avail).0);
        lines.push(Line::from(v));
    }
    TableView { lines, pinned, first_col, shown_cols: shown, total_cols: total }
}
```

Atenção: `avail` já vem sem a largura do recuo (`cont`), como no `render_table` antigo, e o aviso usa `fit(&note, avail)` exatamente como antes. No zoom `cont` é vazio, então nada muda.

3f. Ajuste os dois chamadores: em `TagEnd::Table` (`self.render_table(&t)` → `self.render_table(t)`) e em `render_table_only`:

```rust
    r.render_table(TableBuilder { aligns, rows, cur: Vec::new(), has_header: header, in_head: false });
```

> Nota sobre o rodapé: em CSV/TSV o `data.rs` anexa `N de M linhas · K colunas` **depois** de `render_table_only`, então ele fica fora de `start..end`. É o desejado: o zoom não mostra o rodapé.

- [ ] **Step 4: Rodar os testes e ver passar**

Run: `cargo test 2>&1 | tail -15`
Expected: tudo verde (os testes antigos de tabela, os da Task 1 e os novos). Se `draw_rola_por_coluna_e_mantem_a_primeira` falhar no `c2 `, confira se a coluna 2 realmente saiu (com `first_col = 5` as colunas são 1, 6 e 7).

- [ ] **Step 5: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/render.rs
git commit -m "refactor(render): tabela redesenhável em outra largura e registrada em Rendered.tables (F7.8)"
```

---

### Task 3: Zoom na TUI (`t`, rolagem por coluna)

**Files:**
- Modify: `src/main.rs`
- Test: `src/main.rs` (módulo `tests`)

**Interfaces:**
- Consumes: `render::TableData`, `render::TableSpan`, `render::TableView`, `render::Rendered.tables`, `render::clamp_width`, `Viewport`.
- Produces: `struct Zoom`, `App.zoom: Option<Zoom>`, `App.notice: Option<String>`, `App::open_zoom(&mut self, table: usize)`, `App::open_visible_table(&mut self)`, `App::refresh_zoom(&mut self, width: usize)`, `App::on_zoom_key(&mut self, code: KeyCode)`, `App::draw_zoom(...)`, `fn columns_label(v: &render::TableView) -> String`.

- [ ] **Step 1: Escrever os testes que falham**

No módulo `tests` de `src/main.rs` (depois de `barra_de_status_explica_que_as_teclas_pulam_de_titulo`):

```rust
    fn app_com_tabela(md: &str) -> App {
        let mut app = test_app(Source::Stdin, md);
        app.rerender(60);
        app.view.height = 10;
        app
    }

    fn tecla(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    const TABELA: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";

    fn tabela_larga(n: usize) -> String {
        let h: String = (1..=n).map(|i| format!(" c{i} |")).collect();
        format!("|{h}\n|{}\n|{h}\n", "---|".repeat(n))
    }

    #[test]
    fn t_abre_o_zoom_da_tabela_visivel() {
        let mut app = app_com_tabela(TABELA);
        assert!(!app.on_key(tecla(KeyCode::Char('t'))));
        assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(0));
    }

    #[test]
    fn t_sem_tabela_na_tela_avisa_e_nao_abre() {
        let mut app = app_com_tabela("# só texto\n");
        app.on_key(tecla(KeyCode::Char('t')));
        assert!(app.zoom.is_none());
        assert!(app.notice.as_deref().is_some_and(|n| n.contains("nenhuma tabela")));
        app.on_key(tecla(KeyCode::Char('j')));
        assert!(app.notice.is_none(), "o aviso some na próxima tecla");
    }

    #[test]
    fn t_escolhe_a_tabela_que_aparece_na_tela() {
        let md = format!("{TABELA}\n{}\n{TABELA}", "texto\n\n".repeat(30));
        let mut app = app_com_tabela(&md);
        let segunda = app.rendered.tables[1].start;
        app.view.scroll = segunda.saturating_sub(1);
        app.on_key(tecla(KeyCode::Char('t')));
        assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(1));
    }

    #[test]
    fn esc_e_q_fecham_o_zoom_sem_sair() {
        for k in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('t')] {
            let mut app = app_com_tabela(TABELA);
            app.on_key(tecla(KeyCode::Char('t')));
            assert!(!app.on_key(tecla(k)), "{k:?} no zoom só fecha");
            assert!(app.zoom.is_none());
        }
    }

    #[test]
    fn ctrl_c_sai_mesmo_no_zoom() {
        let mut app = app_com_tabela(TABELA);
        app.on_key(tecla(KeyCode::Char('t')));
        assert!(app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn colunas_rolam_com_h_l_zero_e_cifrao() {
        let mut app = app_com_tabela(&tabela_larga(12));
        app.on_key(tecla(KeyCode::Char('t')));
        app.refresh_zoom(20);
        let first = |app: &App| app.zoom.as_ref().unwrap().first_col;
        assert_eq!(first(&app), 1);

        app.on_key(tecla(KeyCode::Char('l')));
        app.refresh_zoom(20);
        assert_eq!(first(&app), 2);

        app.on_key(tecla(KeyCode::Char('$')));
        app.refresh_zoom(20);
        let v = app.zoom.as_ref().unwrap().drawn.as_ref().map(|d| &d.2).unwrap();
        assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols);

        app.on_key(tecla(KeyCode::Right));
        app.refresh_zoom(20);
        let v = app.zoom.as_ref().unwrap().drawn.as_ref().map(|d| &d.2).unwrap();
        assert_eq!(v.first_col + v.shown_cols - 1, v.total_cols, "não passa da última");

        app.on_key(tecla(KeyCode::Char('0')));
        app.refresh_zoom(20);
        assert_eq!(first(&app), 1);

        app.on_key(tecla(KeyCode::Left));
        app.refresh_zoom(20);
        assert_eq!(first(&app), 1, "não anda para antes da primeira");
    }

    #[test]
    fn rolagem_vertical_do_zoom_nao_passa_do_fim() {
        let linhas: String = (0..40).map(|i| format!("| {i} |\n")).collect();
        let mut app = app_com_tabela(&format!("| n |\n|---|\n{linhas}"));
        app.on_key(tecla(KeyCode::Char('t')));
        app.refresh_zoom(30);
        app.on_key(tecla(KeyCode::Char('G')));
        let z = app.zoom.as_ref().unwrap();
        assert!(z.view.scroll > 0);
        app.on_key(tecla(KeyCode::Char('g')));
        assert_eq!(app.zoom.as_ref().unwrap().view.scroll, 0);
    }

    #[test]
    fn recarregar_com_a_tabela_sumida_fecha_o_zoom() {
        let mut app = app_com_tabela(TABELA);
        app.on_key(tecla(KeyCode::Char('t')));
        app.md = "sem tabela\n".to_string();
        app.rerender(60);
        assert!(app.zoom.is_none());
    }

    #[test]
    fn rerender_com_a_tabela_ainda_presente_mantem_o_zoom_e_refaz_o_desenho() {
        let mut app = app_com_tabela(TABELA);
        app.on_key(tecla(KeyCode::Char('t')));
        app.refresh_zoom(30);
        app.rerender(60);
        let z = app.zoom.as_ref().expect("continua aberto");
        assert!(z.drawn.is_none(), "o cache do desenho é descartado");
    }

    #[test]
    fn rotulo_de_colunas() {
        let md = tabela_larga(12);
        let out = render::render(&md, 20);
        let data = &out.tables[0].data;
        assert_eq!(columns_label(&data.draw(200, 0)), "todas as 12 colunas");
        assert_eq!(columns_label(&data.draw(20, 0)), "colunas 1 e 2–3 de 12");
        let so_uma = render::render("| a |\n|---|\n| 1 |\n", 20);
        assert_eq!(columns_label(&so_uma.tables[0].data.draw(20, 0)), "1 coluna");
    }
```

(Se `render::render` for `pub`, ótimo; se o módulo de testes de `main.rs` só enxergar `render_with`, use `render::render_with(&md, &render::RenderOpts { width: 20, images: None })`. O CLAUDE.md diz que `render(md, width)` é o atalho público.)

- [ ] **Step 2: Rodar os testes e ver falhar**

Run: `cargo test --bin mdv t_abre 2>&1 | tail -15`
Expected: erro de compilação (`zoom`, `notice`, `refresh_zoom`, `columns_label` não existem).

- [ ] **Step 3: Implementar o estado e as teclas**

Em `src/main.rs`, depois de `SearchState`, acrescente:

```rust
/// Tela de zoom: uma tabela desenhada na largura inteira do terminal.
struct Zoom {
    /// Índice em `Rendered.tables`.
    table: usize,
    /// Primeira coluna rolável pedida (a coluna 0 fica sempre; `draw` normaliza).
    first_col: usize,
    /// Rolagem do corpo da tabela, sem as linhas fixas do topo.
    view: Viewport,
    /// Último desenho: (largura, `first_col` usado, resultado).
    drawn: Option<(usize, usize, render::TableView)>,
}

impl Zoom {
    fn new(table: usize) -> Self {
        Zoom { table, first_col: 0, view: Viewport::default(), drawn: None }
    }

    /// Linhas do corpo (tudo que não é fixo no topo).
    fn body_total(&self) -> usize {
        self.drawn.as_ref().map_or(0, |(_, _, v)| v.lines.len().saturating_sub(v.pinned))
    }
}

/// "todas as N colunas" ou "colunas 1 e A–B de N" (1 é a coluna fixa).
fn columns_label(v: &render::TableView) -> String {
    if v.total_cols == 1 {
        return "1 coluna".to_string();
    }
    if v.shown_cols >= v.total_cols {
        return format!("todas as {} colunas", v.total_cols);
    }
    if v.shown_cols == 1 {
        return format!("coluna 1 de {}", v.total_cols);
    }
    let (from, to) = (v.first_col + 1, v.first_col + v.shown_cols - 1);
    format!("colunas 1 e {from}–{to} de {}", v.total_cols)
}
```

Em `App` acrescente os campos `zoom: Option<Zoom>` e `notice: Option<String>`, ambos `None` em `App::new`.

Métodos de `App` (junto de `goto_match`):

```rust
    fn open_zoom(&mut self, table: usize) {
        if table >= self.rendered.tables.len() {
            self.notice = Some(format!(
                "tabela {} não existe (há {})",
                table + 1,
                self.rendered.tables.len()
            ));
            return;
        }
        self.notice = None;
        self.zoom = Some(Zoom::new(table));
    }

    /// Abre o zoom da primeira tabela que aparece na tela.
    fn open_visible_table(&mut self) {
        let (top, bottom) = (self.view.scroll, self.view.scroll + self.view.height);
        match self.rendered.tables.iter().position(|t| t.end > top && t.start < bottom) {
            Some(i) => self.open_zoom(i),
            None => self.notice = Some("nenhuma tabela na tela".to_string()),
        }
    }

    /// Redesenha a tabela do zoom se a largura ou a coluna mudaram.
    fn refresh_zoom(&mut self, width: usize) {
        let Some(z) = self.zoom.as_mut() else { return };
        let Some(span) = self.rendered.tables.get(z.table) else { return };
        let fresh = matches!(&z.drawn, Some((w, c, _)) if *w == width && *c == z.first_col);
        if !fresh {
            let v = span.data.draw(width, z.first_col);
            z.first_col = v.first_col;
            z.drawn = Some((width, v.first_col, v));
        }
    }

    fn on_zoom_key(&mut self, code: KeyCode) {
        let Some(z) = self.zoom.as_mut() else { return };
        let body = z.body_total();
        let h = z.view.height;
        let page = isize::try_from(h.saturating_sub(1).max(1)).unwrap_or(isize::MAX);
        let half = isize::try_from((h / 2).max(1)).unwrap_or(isize::MAX);
        match code {
            KeyCode::Esc | KeyCode::Char('q' | 't') => self.zoom = None,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => z.view.scroll_by(1, body),
            KeyCode::Char('k') | KeyCode::Up => z.view.scroll_by(-1, body),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => z.view.scroll_by(page, body),
            KeyCode::Char('b') | KeyCode::PageUp => z.view.scroll_by(-page, body),
            KeyCode::Char('d') => z.view.scroll_by(half, body),
            KeyCode::Char('u') => z.view.scroll_by(-half, body),
            KeyCode::Char('g') | KeyCode::Home => z.view.scroll_to(0, body),
            KeyCode::Char('G') | KeyCode::End => z.view.scroll_to(usize::MAX, body),
            KeyCode::Char('l') | KeyCode::Right => z.first_col = z.first_col.saturating_add(1),
            KeyCode::Char('h') | KeyCode::Left => z.first_col = z.first_col.saturating_sub(1),
            KeyCode::Char('0') => z.first_col = 0,
            KeyCode::Char('$') => z.first_col = usize::MAX,
            _ => {}
        }
    }
```

Em `on_key`, logo depois do tratamento de `Ctrl-C` e antes do bloco `if let Some(buf) = self.search.input.take()`:

```rust
        if self.zoom.is_some() {
            self.on_zoom_key(k.code);
            return false;
        }
        self.notice = None;
```

e no `match k.code` principal acrescente, antes de `KeyCode::Char('m')`:

```rust
            KeyCode::Char('t') => self.open_visible_table(),
```

Em `rerender`, depois de `self.search.refresh(...)`:

```rust
        if let Some(z) = &mut self.zoom {
            if z.table >= self.rendered.tables.len() {
                self.zoom = None;
            } else {
                z.drawn = None;
            }
        }
```

(`z` empresta `self.zoom`; para atribuir `self.zoom = None` dentro do `if let`, estruture como `let gone = self.zoom.as_ref().is_some_and(|z| z.table >= n); if gone { self.zoom = None } else if let Some(z) = &mut self.zoom { z.drawn = None }`.)

- [ ] **Step 4: Implementar o desenho do zoom**

Em `App::draw`, depois do bloco que chama `self.rerender(w)` e antes de calcular `inner`:

```rust
        if self.zoom.is_some() {
            self.draw_zoom(f, content, status);
            return;
        }
```

E o método:

```rust
    fn draw_zoom(&mut self, f: &mut Frame, content: Rect, status: Rect) {
        self.refresh_zoom(render::clamp_width(usize::from(content.width)));
        let Some(z) = self.zoom.as_mut() else { return };
        let Some((_, _, tv)) = &z.drawn else { return };
        let pinned = tv.pinned.min(tv.lines.len());
        z.view.height = usize::from(content.height).saturating_sub(pinned).max(1);
        let body = tv.lines.len() - pinned;
        z.view.scroll = z.view.scroll.min(z.view.max_scroll(body));
        let start = pinned + z.view.scroll;
        let end = (start + z.view.height).min(tv.lines.len());
        let mut visible: Vec<Line<'static>> = tv.lines[..pinned].to_vec();
        visible.extend_from_slice(&tv.lines[start..end]);
        f.render_widget(Paragraph::new(visible), content);

        let bar = Style::default().bg(Color::Rgb(59, 66, 82)).fg(Color::Rgb(229, 233, 240));
        let left = format!(
            " {}  tabela {}/{}  {}",
            self.name,
            z.table + 1,
            self.rendered.tables.len(),
            columns_label(tv)
        );
        let right = "j/k rolar  h/l colunas  0/$ extremos  Esc voltar ";
        let width = usize::from(status.width);
        let (lw, rw) = (left.width(), right.width());
        let text = if lw + rw < width { format!("{left}{}{right}", " ".repeat(width - lw - rw)) } else { left };
        f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
    }
```

Use `Rect`/`Style`/`Color` já importados em `main.rs` (são os mesmos de `draw`). Se o borrow checker reclamar de `self.name` enquanto `z` está emprestado, copie `let name = self.name.clone();` e `let ntables = self.rendered.tables.len();` antes de `let Some(z) = ...`.

Mostre o aviso na barra normal: em `status_left`, no fim, antes de devolver `s`:

```rust
        if let Some(n) = &self.notice {
            let _ = write!(s, "  {n}");
        }
```

Atualize o `right` de `status_line` para `"j/k rolar  [ ] pular título  / buscar  t tabela  m fonte  q sair "` (o teste `barra_de_status_explica_que_as_teclas_pulam_de_titulo` continua verdadeiro).

- [ ] **Step 5: Rodar os testes e ver passar**

Run: `cargo test 2>&1 | tail -20`
Expected: tudo verde. Depois valide à mão num terminal real (a TUI não roda sem TTY): `cargo run --release -- samples/tabelas.md`, role até uma tabela, tecle `t`, `l`, `$`, `0`, `j`, `Esc`.

- [ ] **Step 6: Clippy e commit**

Run: `cargo clippy --all-targets 2>&1 | tail -15`

```bash
git add src/main.rs
git commit -m "feat(tui): zoom de tabela com t, cabeçalho fixo e rolagem por coluna (F7.8)"
```

---

### Task 4: `--table N`, ajuda, amostra e documentação

**Files:**
- Modify: `src/cli.rs`, `src/main.rs` (`HELP`, `App::new`, `App::draw`), `tests/cli.rs`, `samples/tabelas.md`, `README.md`, `PLANO.md`, `docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`
- Test: `src/cli.rs`, `src/main.rs`, `tests/cli.rs`

**Interfaces:**
- Consumes: `App::open_zoom(&mut self, table: usize)` (Task 3).
- Produces: `Cli.table: Option<usize>` (1-based, já validado ≥ 1), `App.open_table: Option<usize>` (0-based, pendente até a primeira renderização).

- [ ] **Step 1: Escrever os testes que falham**

Em `src/cli.rs` (módulo `tests`):

```rust
    #[test]
    fn table_aceita_numero_a_partir_de_1() {
        assert_eq!(run(&["--table", "2", "a.md"]).unwrap().table, Some(2));
        assert_eq!(run(&["a.md"]).unwrap().table, None);
    }

    #[test]
    fn table_invalido_e_erro_de_uso() {
        for v in ["0", "-1", "x", ""] {
            let e = run(&["--table", v, "a.md"]).unwrap_err();
            assert!(e.contains("--table"), "{e}");
        }
        assert!(run(&["--table"]).is_err());
    }
```

Em `src/main.rs` (módulo `tests`):

```rust
    fn app_com_cli(args: &[&str], md: &str) -> App {
        let cli = match cli::parse(args.iter().map(|s| s.to_string())) {
            Ok(cli::Parsed::Run(c)) => c,
            _ => panic!("cli de teste"),
        };
        App::new(Source::Stdin, md.to_string(), 80, &cli)
    }

    #[test]
    fn table_n_abre_o_zoom_na_primeira_renderizacao() {
        let md = format!("{TABELA}\ntexto\n\n{TABELA}");
        let mut app = app_com_cli(&["--table", "2", "-"], &md);
        app.rerender(60);
        app.open_pending_table();
        assert_eq!(app.zoom.as_ref().map(|z| z.table), Some(1));
        assert!(app.open_table.is_none());
    }

    #[test]
    fn table_alem_do_que_existe_avisa_e_nao_abre() {
        let mut app = app_com_cli(&["--table", "5", "-"], TABELA);
        app.rerender(60);
        app.open_pending_table();
        assert!(app.zoom.is_none());
        assert!(app.notice.as_deref().is_some_and(|n| n.contains("tabela 5 não existe (há 1)")));
    }
```

Em `tests/cli.rs`:

```rust
#[test]
fn csv_com_celula_longa_quebra_em_vez_de_truncar() {
    let frase = "uma descricao bem longa que precisa quebrar em varias linhas dentro da celula final";
    let out = mdv(&["csv", "-p", "-w", "40", "-"], &format!("id,texto\n1,{frase}\n"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains('…'), "{text}");
    assert!(text.contains("final"), "{text}");
    assert!(text.lines().all(|l| l.chars().count() <= 40), "{text}");
}

#[test]
fn markdown_com_celula_longa_quebra_em_plain() {
    let md = "| a | b |\n|---|---|\n| 1 | palavra1 palavra2 palavra3 palavra4 palavra5 palavra6 palavra7 |\n";
    let out = mdv(&["-p", "-w", "30", "-"], md);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("palavra7") && !text.contains('…'), "{text}");
}

#[test]
fn table_zero_e_erro_de_uso() {
    let out = mdv(&["--table", "0", "-"], "# x\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--table"));
}

#[test]
fn table_e_ignorado_em_plain() {
    let out = mdv(&["-p", "--table", "9", "-"], "| a |\n|---|\n| 1 |\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains('┌'));
}
```

- [ ] **Step 2: Rodar os testes e ver falhar**

Run: `cargo test 2>&1 | tail -20`
Expected: erro de compilação (`table`, `open_table`, `open_pending_table` não existem).

- [ ] **Step 3: Implementar**

`src/cli.rs`: campo `pub table: Option<usize>,` em `Cli`, `table: None,` no literal de `parse`, e o braço:

```rust
            "--table" => {
                let v = value(&mut args, &a)?;
                cli.table = Some(match v.parse::<usize>() {
                    Ok(n) if n >= 1 => n,
                    _ => return Err(format!("--table inválido: {v} (esperado um número a partir de 1)")),
                });
            }
```

`src/main.rs`: campo `open_table: Option<usize>` em `App` (`cli.table.map(|n| n - 1)` em `App::new`), o método

```rust
    /// Abre o zoom pedido por `--table`, assim que há um documento renderizado.
    fn open_pending_table(&mut self) {
        if let Some(i) = self.open_table.take() {
            self.open_zoom(i);
        }
    }
```

e a chamada em `App::draw`, logo depois do bloco `if self.dirty || w != self.render_width { self.rerender(w); }`:

```rust
        self.open_pending_table();
```

`HELP`: na lista de OPÇÕES, depois de `--latest`:

```
        --table <N>    abre direto no zoom da N-ésima tabela (só no modo interativo)
```

e nas TECLAS, depois de `m`:

```
    t                  zoom da tabela na tela (h/l colunas, Esc volta)
```

Amostra: em `samples/tabelas.md` acrescente no fim:

```markdown

## Célula longa

| Arquivo | Descrição |
|---|---|
| `src/render.rs` | Máquina de estados sobre os eventos do pulldown-cmark: as linhas já saem quebradas na largura pedida, então quem consome não faz wrap. |
| `https://example.com/um/caminho/muito/longo/sem/espacos/que/nao/cabe/na/coluna/index.html` | URL sem espaços quebra por caractere |
```

Documentação:
- `README.md`: linha 25 vira "**Tabelas que cabem na tela.** Colunas se ajustam à largura disponível; células largas quebram em várias linhas (até 8, depois `…`). Na TUI, `t` abre a tabela na largura inteira do terminal, com cabeçalho e primeira coluna fixos e `h`/`l` para rolar as colunas; `--table N` abre direto numa delas."; no trecho de CSV (linha 149) troque "use `--cols` para tabelas largas, pois não há rolagem horizontal" por "para tabelas com muitas colunas use `t` (zoom) ou `--cols`"; em "Limitações conhecidas" apague o item "Células de tabela largas são truncadas com `…`." e, em "Uso", documente `--table N` e a tecla `t` onde as outras teclas estão listadas.
- `PLANO.md`: marque F7.7 e F7.8 como `[x]` ao fim, e acrescente em F7.8 a frase "Plano detalhado: [`docs/superpowers/plans/2026-10-15-fase-7-tabelas-quebra-e-zoom.md`](docs/superpowers/plans/2026-10-15-fase-7-tabelas-quebra-e-zoom.md)." também em F7.7.
- Spec (`docs/superpowers/specs/2026-10-13-fase-7-produtividade-design.md`), seção F7.8: troque "Scroll horizontal: ... `h`/`l` rolam, `0`/`$` vão ao início e ao fim; cabeçalho e primeira coluna ficam fixos" por "rolagem por coluna (`h`/`l`, `0`/`$`), com cabeçalho e primeira coluna fixos"; apague a linha "CSV/TSV e JSON em tabela: ... alterna o modo tela cheia sem rodapé" e escreva "CSV/TSV e JSON em tabela: `t` funciona como no Markdown (o rodapé fica fora do zoom)"; em `--table N` troque "(erro de uso, código 2)" por "(`--table 0` é erro de uso, código 2; N maior que o número de tabelas mostra aviso na barra)".

- [ ] **Step 4: Rodar tudo**

Run: `cargo test 2>&1 | tail -15 && cargo clippy --all-targets 2>&1 | tail -10`
Expected: verde, sem avisos novos.

Validação manual (sem TTY o modo `-p` basta para o wrap):

```bash
cargo run --release -- -p -w 50 samples/tabelas.md | tail -25
cargo run --release -- -p -w 40 samples/vendas.csv
cargo run --release -- --table 3 samples/tabelas.md   # TUI: já abre no zoom da 3ª tabela; Esc volta
```

Expected: a seção "Célula longa" aparece quebrada, sem `…`, com bordas contínuas; o CSV não perde texto; a 3ª tabela (muitas colunas) abre com `colunas 1 e 2–N de 10` na barra e `l`/`$` andam coluna a coluna.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs tests/cli.rs samples/tabelas.md README.md PLANO.md docs/superpowers
git commit -m "feat: --table N, ajuda, amostra e docs de F7.7 e F7.8"
```

---

## Self-Review

**Cobertura da spec:**
- F7.7 quebra em palavra e em caractere, teto de 8 linhas com `…`, alinhamento por linha da célula, bordas `│` em todas as linhas visuais, separador só após o cabeçalho, Markdown e CSV/TSV, `compute_column_widths` intacto → Task 1.
- F7.8 tecla `t` com alvo = tabela na tela, tela sem recuo na largura total, `Esc`/`q` voltam, cabeçalho e primeira coluna fixos, rolagem por coluna (`h`/`l`/`0`/`$`), `Rendered.tables` como fundação → Tasks 2 e 3; `--table N` → Task 4.
- Os três refinamentos da spec (rolagem por coluna, CSV/JSON usam o mesmo `t`, `--table` fora do intervalo vira aviso) estão declarados em "Decisões que refinam a spec" e aplicados na documentação da Task 4.
- Interação com a F7.6: os tetos de 200 colunas e 500 caracteres por célula valem antes do wrap e não conflitam; a F7.6 mexe em `data.rs` e em `finish_table`, aqui só tocamos `render.rs` e `main.rs`. Se a F7.6 já tiver sido feita, o único ponto de contato é `HELP`/`status_line` (ambos têm linhas acrescentadas; resolva o merge mantendo as duas).

**Placeholders:** nenhum "TBD"/"similar à Task N"; os trechos de código estão completos.

**Consistência de tipos:** `draw_table_row` muda de `Line` para `Vec<Line>` na Task 1 e de `&[Vec<Piece>]` para `&[&[Piece]]` na Task 2 (a Task 1 só altera o corpo e o retorno; a assinatura de entrada muda na Task 2, Step 3d, e é a única que chama `.copied()`). `natural_widths` muda de `ncols` para `idx` na Task 2 e só `layout_table` a chama. `TableView.first_col` é o mesmo conceito em `layout_table`, em `Zoom.first_col` e em `columns_label`. `Zoom.drawn` é `Option<(usize, usize, TableView)>` em todos os usos. `App::open_zoom(usize)` recebe índice 0-based; `Cli.table` é 1-based e `App.open_table` é 0-based.

**Review Focus → testes:** URL/hash (`palavra_maior_que_a_coluna_quebra_por_caractere`, amostra); 3 milhões de caracteres (`tabela_larga_renderiza_em_tempo_limitado` + `celula_enorme_respeita_o_teto`); célula vazia e CJK (`todas_as_linhas_da_tabela_tem_a_mesma_largura`); citação e lista (`tabela_em_citacao_e_lista_registra_o_intervalo_com_recuo`); uma coluna e colunas demais (`draw_com_uma_coluna_nao_estoura_o_indice`, `draw_rola_por_coluna_e_mantem_a_primeira`, `draw_cabe_em_qualquer_largura_pedida`); reload com tabela sumida (`recarregar_com_a_tabela_sumida_fecha_o_zoom`); `t` sem tabela e `--table` inválido (`t_sem_tabela_na_tela_avisa_e_nao_abre`, `table_alem_do_que_existe_avisa_e_nao_abre`, `table_zero_e_erro_de_uso`).
