# Fase 7 — Produtividade com IA: leitura e reaproveitamento (design)

Origem: `PLANO.md`, seção "Fase 7". Este documento fixa o desenho; o plano passo a passo sai depois da aprovação.

## Objetivo e uso alvo

Quem lê no terminal Markdown gerado por agentes e LLMs (planos, specs, respostas longas) precisa:

1. **Reaproveitar** trechos (código, seções) sem selecionar com o mouse: copiar na TUI e extrair em pipe.
2. **Acompanhar** um arquivo que o agente edita: ver o que mudou e o andamento das tarefas.
3. **Navegar** documentos longos dobrando seções.

Critério de sucesso: cada item tem teste automatizado, funciona sem rede nem subprocesso, e a saída nunca contém escapes além do SGR gerado pelo `mdv` (exceção deliberada: OSC 52 do `y`/`Y`, emitido só pela TUI).

## Pré-requisitos e achados do código

- Dependem da Fase 6: F6.1 (`src/cli.rs` e flags), F6.6 (`OutlineEntry`, `--toc`) e F6.7 (`--follow`, que também recarrega o conteúdo e deve passar pelo mesmo ponto de F7.1).
- Task list já é renderizada (`☑`/`☐`, `ListStack::set_task_marker`, `Options::ENABLE_TASKLISTS`). F7.2 só precisa **contar**, não desenhar.
- `Rendered` hoje tem `lines` e `headings`. Não há mapa linha renderizada → linha do fonte, nem registro de blocos de código. Vários itens precisam disso.
- `App::reload` substitui `md` e marca `dirty`; é o ponto único onde F7.1 compara versões.

## Fundação compartilhada (`render.rs`)

`Rendered` ganha:

```rust
pub struct CodeSpan { pub start: usize, pub end: usize, pub lang: String, pub text: String } // linhas renderizadas [start, end)
pub struct TaskItem { pub line: usize, pub done: bool }
// Rendered: pub line_src: Vec<usize>      // linha (0-based) do fonte onde cada linha renderizada começa
//           pub code_blocks: Vec<CodeSpan>
//           pub tasks: Vec<TaskItem>
```

- `line_src` vem de `Parser::into_offset_iter` (offset de byte → número de linha). Linhas de continuação (quebra por largura) herdam a do bloco; linhas geradas sem origem herdam a anterior.
- `CodeSpan.text` é o código **cru** (já sanitizado), não o realçado.
- Seções: intervalo de um título até o próximo de nível igual ou menor (ou fim do documento). Função pura `section_range(outline, idx) -> (usize, usize)` sobre `OutlineEntry` (F6.6), em termos de linhas renderizadas e, via `line_src`, de linhas do fonte.

Esta fundação é a Task 1 do plano e não muda nenhuma saída existente.

## F7.4 Copiar bloco ou seção (`y` / `Y`)

- **Alvo:** referência é a primeira linha visível. `y` copia o `CodeSpan` que contém essa linha; se nenhum, o primeiro que cruza a janela; se nenhum, status "nenhum bloco de código".
- `Y` copia o **Markdown cru** da seção que contém a linha de referência (fatiado do fonte via `line_src`), com o título.
- **Mecanismo:** OSC 52 (`ESC ] 52 ; c ; <base64> BEL`) escrito no terminal da TUI. Base64 implementado em ~15 linhas em `src/clip.rs` (evita dependência). Não usa `xclip`/`pbcopy`.
- **Limites:** texto copiado ≤ 64 KiB (maior: status "grande demais (N KiB)", nada é enviado). O texto passa por `sanitize` antes do base64; o payload é só base64, então não há injeção.
- **Feedback:** mensagem de status "copiado: N linhas" por 2 s. Limitação documentada: tmux exige `set -g set-clipboard on`; terminais sem OSC 52 ignoram em silêncio, sem como detectar.
- **Teste:** codificação base64 (vetores RFC 4648), escolha do alvo, limite de tamanho e a sequência emitida.

## F7.5 Extração em pipe (`--extract-code`, `--section`)

Novo módulo `src/extract.rs`, sem renderização; trabalha sobre o fonte com `pulldown-cmark`.

- `mdv --extract-code [--lang L] [-n N] ARQ`: imprime o texto cru dos blocos de código. `--lang` filtra por linguagem (comparação sem caixa); `-n` escolhe o N-ésimo (1-based) **depois** do filtro. Vários blocos saem separados por uma linha em branco.
- `mdv --section TÍTULO ARQ`: imprime o Markdown cru da seção. Casamento: igualdade sem caixa; se não houver, substring única; se ambígua, erro listando os candidatos.
- Combináveis: `--section` restringe o escopo e `--extract-code` extrai dentro dele.
- Saída sem cor e sem formatação, sempre sanitizada. Sem correspondência: mensagem em português em stderr e exit code 1; flag inválida: exit code 2 (convenção de F6.1).
- **Teste:** `tests/cli.rs` com arquivos temporários: filtro por linguagem, `-n` fora do intervalo, título ambíguo, seção aninhada, bloco indentado, ESC no código.

## F7.2 Progresso de task lists

- `tasks` vem da fundação (um `TaskItem` por `TaskListMarker`).
- Rodapé da TUI mostra `☑ feitas/total` do documento, e a seção atual quando há tarefas nela. Sem tarefas, nada aparece.
- `--toc` acrescenta ` [feitas/total]` aos títulos cujas seções têm tarefas (a contagem de um título inclui subseções).
- Sem mudança no desenho dos itens.
- **Teste:** contagem em lista aninhada, item marcado dentro de citação, seção sem tarefas, formato do `--toc`.

## F7.3 Fold por título (`z` / `Z`)

- Estado em `App`: `folded: HashSet<(String, usize)>` — chave = (título, n-ésima ocorrência do título). Sobrevive a reload e a mudança de largura, ao contrário de índices.
- `display: Vec<usize>` (índices de `rendered.lines` visíveis) é recalculado em `rerender` e a cada fold. Rolagem, desenho e navegação `]`/`[` passam a operar sobre `display`. O título dobrado ganha o sufixo ` ▸ N linhas`.
- `z` alterna a seção que contém a linha de referência; `Z` dobra todas se alguma estiver aberta, senão abre todas.
- Busca continua sobre todas as linhas; ao saltar para uma ocorrência dentro de seção dobrada, a seção é aberta.
- Efeito colateral aceito: o `scroll` do `Viewport` passa a indexar `display`, não `lines`. A conversão fica encapsulada em `Viewport`/`App`.
- **Teste:** `display` com seções aninhadas, dobrar título dentro de título dobrado, chave estável após reload, busca abrindo a dobra.

## F7.1 Marcação de mudanças no reload

- Em `App::set_md(novo)` (usado por `reload` e por `--follow`): compara linhas do fonte antigo e do novo com o crate `similar` (v2, sem dependências pesadas) e guarda `changed: BTreeSet<usize>` com as linhas **novas** (inserções e substituições).
- Cada linha renderizada cuja `line_src` está em `changed` recebe, no desenho da TUI, `▌` amarelo na margem esquerda (uso da `TUI_MARGIN`, não toca `render.rs`). O `--plain` não muda.
- Sem marcas na carga inicial. As marcas persistem até a tecla `c` (limpa) ou o próximo reload (substitui, sempre relativas à versão imediatamente anterior).
- `}` / `{` saltam para o próximo / anterior trecho alterado.
- Limite: acima de 20 000 linhas no fonte, não calcula o diff (status "documento grande: sem marcação"). Custo O(ND) abaixo disso.
- **Teste:** inserção, edição e remoção; marcas após reload duplo; mapeamento via `line_src` com linha quebrada em duas; `c` limpa; salto.

## F7.6 JSON em tabela e esquema

Origem: o `mdv json` só indentava e colorizava o JSON; faltavam vistas estruturadas. Plano passo a passo: `docs/superpowers/plans/2026-10-14-fase-7-json-tabela-esquema.md`. Não depende da fundação acima.

- **Vistas:** `Text` (a atual), `Table` e `Schema`, escolhidas por `--as auto|text|table|schema` (só vale para JSON; em outro modo é erro de uso, código 2) e alternadas na TUI por `v` (tabela → esquema → texto). A barra de status mostra `[tabela]`, `[esquema]` ou `[texto]` quando a vista foi escolhida.
- **Automático:** lista de objetos (e JSONL) abre como tabela; qualquer outra forma abre como texto. `--path` é aplicado antes da vista.
- **Tabela:** colunas = união das chaves na ordem em que aparecem; objetos aninhados achatados como `a.b.c` até 4 níveis; listas e níveis além do 4º viram JSON compacto na célula; `--head` e `--cols` valem; colunas numéricas à direita; rodapé `N de M itens · K colunas (+X omitidas)`. Objeto único vira tabela `chave | valor`; lista de escalares vira a coluna `valor`; outras formas (lista de listas, lista mista, vazio) dão a mensagem "não cabe em tabela".
- **Esquema:** texto indentado de tipos (`objeto`, `lista · N × tipo`, `texto`, `número`, `booleano`, `nulo`), `chave?` quando a chave falta em parte dos itens, `a | b` para tipos mistos. Analisa uma amostra dos 1000 primeiros itens de cada lista.
- **Tetos (valem também para CSV/TSV):** 200 colunas, 500 caracteres por célula, esquema com 8 níveis, 200 chaves por objeto e 2000 linhas. O que passa disso é contado e sinalizado com `…` ou `+N omitidas`.
- **Fora do escopo:** vista em árvore recolhível (fica com o fold da F7.3, pois ambos mexem no `Viewport`) e vista chave/valor (`--path` mais `grep` cobrem o uso).
- **Teste:** tabela (união de chaves, aninhado, JSONL, `--head`/`--cols`, formas que não cabem), esquema (opcional, tipos mistos, amostra grande, teto), controle em chave e valor, tetos de colunas e células, `--as` inválido, tecla `v` e rótulo na barra.

## F7.7 Quebra de linha em células de tabela

Origem: célula mais larga que a coluna era cortada por `fit()` com `…`, e sem scroll horizontal o resto do texto ficava ilegível. Cortar o começo (mostrar o fim) foi descartado: começo e fim costumam ter informação, e só ajuda em caminhos e nomes com sufixo.

- **Mudança:** em `render.rs`, `draw_table_row` vira função que devolve `Vec<Line>`. Cada célula é quebrada por `wrap_cell(cell, w, max_lines)` (derivada de `fit`); a altura da linha é o máximo entre as células e as mais curtas são preenchidas com espaços.
- **Quebra:** em palavra; em caractere quando uma palavra é maior que a coluna. Estilos (negrito, código, link) seguem por trecho.
- **Teto:** 8 linhas por célula; ao estourar, `…` ao fim da última. Alinhamento aplicado em cada linha da célula.
- **Bordas:** `│` em todas as linhas visuais; separador só após o cabeçalho.
- **Escopo:** Markdown e CSV/TSV (mesmo desenho via `render_table_only`). `compute_column_widths` e a omissão de colunas (`… +N colunas`) não mudam. Os tetos da F7.6 continuam valendo antes do wrap.
- **Teste:** célula longa quebrada; palavra maior que a coluna; teto com `…`; alinhamento à direita em célula quebrada; todas as linhas da tabela com a mesma largura; CSV com célula longa em `-p -w 40`.

## F7.8 Zoom de tabela

Origem: mesmo com wrap, tabela larga em terminal estreito fica espremida e recuada por listas/citações; o leitor quer ver a tabela inteira como numa tela só dela.

- **Tecla `t`:** abre a tabela cujo bloco contém a primeira linha visível (mesma regra de alvo de `y`/`z`, sem cursor). Tela própria, sem recuo e com a largura total do terminal; `Esc`/`q` volta ao ponto anterior. Não é zoom de fonte: é reflow na largura total.
- **Scroll horizontal:** se a largura natural ainda não cabe, rolagem por coluna (`h`/`l`, `0`/`$`), com cabeçalho e primeira coluna fixos. Escopo do scroll horizontal limitado ao zoom de tabela.
- **Fundação:** `Rendered` passa a guardar `tables` (intervalo de linhas e as linhas de dados já sanitizadas de cada tabela), parecido com `code_blocks`. O zoom chama `render_table_only` com a largura própria.
- **CSV/TSV e JSON em tabela:** `t` funciona como no Markdown (o rodapé fica fora do zoom).
- **`--table N`:** abre direto no zoom da N-ésima tabela; `--plain` ignora a flag.
- **Depende de** F7.7. **Risco:** scroll horizontal mexe no `Viewport`; manter isolado do vertical.
- **Teste:** abrir, fechar e voltar ao mesmo ponto; tabela dentro de lista; rolagem horizontal com colunas fixas; `--table` fora do intervalo (`--table 0` é erro de uso, código 2; N maior que o número de tabelas mostra aviso na barra); `t` sem tabela visível mostra aviso na barra.

## Ordem de implementação

1. Fundação (`line_src`, `code_blocks`, `tasks`).
2. F7.4 `y`/`Y` (`clip.rs`) · 3. F7.5 (`extract.rs`) · 4. F7.2 · 5. F7.3 · 6. F7.1.
7. F7.6 e F7.7 não dependem de nada acima e podem ser feitos antes de todos. 8. F7.8 logo depois de F7.7.

## Dependências novas

Só `similar = "2"` (F7.1). Base64 é local.

## Decisões fixadas e riscos

- Alvo de `y`/`z` = primeira linha visível (sem cursor de linha): evita criar modo de seleção. Se isso se mostrar incômodo, o cursor fica para uma fase futura.
- Dobrar e `line_src` mexem em `Viewport`/`Rendered`, as áreas mais exercitadas; por isso a fundação vem primeiro e o fold depois dos itens menores.
- Não-objetivos: seleção livre de texto, clipboard via comando externo, histórico de versões além da anterior, diff em `--plain`.
