# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Comandos

```bash
cargo build --release
cargo run --release -- sample.md     # TUI interativa
cargo run --release -- -p sample.md  # modo --plain (imprime ANSI, sem TUI)
cargo run --release -- -p -w 60 sample.md  # largura fixa
cat README.md | cargo run --release  # stdin
cargo run --release -- -p samples/llm.md       # front matter, alertas, <think>...
cargo run --release -- -p samples/img.md       # imagem local em meio-bloco
cargo run --release -- -p csv samples/dados.csv   # também: json, tsv
cargo run --release -- -p json samples/itens.json   # tabela; --as schema para o esquema
cargo clippy
```

`cargo test` roda os testes unitários (`render.rs`, `main.rs`, `cli.rs`, `image.rs`, `net.rs`, `mermaid.rs`, `stream.rs`, `data.rs`) e `tests/cli.rs` (executa o binário). Para validar mudanças de renderização, também use `-p` com `sample.md` ou com `samples/` — funciona sem terminal interativo.

O projeto, README e mensagens de erro/ajuda são em português.

## Memória: limitar a aplicação, nunca estourar a máquina

Nenhum teste (nem execução manual) pode alocar memória sem teto. Entrada hostil ou gigante se resolve **limitando o uso de memória do `mdv`** (tetos de bytes, de itens, de caracteres por chave/célula, de profundidade), nunca rodando para ver se aguenta.

- Antes de escrever ou rodar um teste com entrada grande, calcule o pior caso de memória (inclusive o que o código derivado gera: concatenar o prefixo de uma chave de N caracteres para cada um de M filhos custa N×M). Se passar de algumas dezenas de MiB, **não rode**: corrija o limite no código primeiro e só então teste com tamanhos pequenos que provem o limite (por exemplo, teto reduzido por constante ou parâmetro de teste).
- Entradas de teste de estresse ficam em dezenas de MiB no máximo; prefira provar o teto com um valor um pouco acima dele.
- Se a ferramenta for rodar algo potencialmente pesado, use `ulimit -v` (por exemplo `(ulimit -v 2000000; cargo test ...)`) para o processo morrer sozinho em vez de travar o terminal.
- O `mdv` já tem esse fail-safe (`src/mem.rs`): novo laço que cresça com a entrada deve chamar `mem::check()`/`mem::exceeded()`.

## Arquitetura

Um único pipeline: entrada (Markdown, JSON, CSV/TSV) → `Vec<Line>` do ratatui → (TUI | ANSI).

- `src/render.rs`: `render_with(md, &RenderOpts { width, images }) -> Rendered { lines, headings, outline }` (`render(md, width)` é o atalho sem imagens). Máquina de estados sobre eventos do `pulldown-cmark` (`Renderer::handle/start/end`), com o estado dividido em `InlineBuffer` (texto em linha, estilos, links), `ListStack`, `CodeBlock` e `TableBuilder`. As linhas saem **já quebradas na largura pedida** (quebra com `unicode-width`, em `wrap`/`fit`/`split_by_width`), então quem consome não faz wrap. `headings` guarda o índice em `lines` de cada título (navegação `]`/`[`); `outline` guarda nível e texto (`--toc`). Blocos de código usam `syntect` (sem onig; syntax/theme em `OnceLock` via `hl()`). `to_ansi` converte as mesmas linhas para SGR no modo `--plain`. Também entende front matter YAML, alertas GFM, notas de rodapé, `<details>`/`<summary>` e `<think>`. Pontos de entrada extras: `render_code_only` (fonte crua, tecla `m`), `render_table_only` (CSV/TSV) e `error_rendered`. Todo texto vindo do documento passa por `sanitize` (caracteres de controle viram U+FFFD). `Rendered` também guarda `line_src` (linha do fonte de cada linha renderizada, vazio fora do Markdown), `code_blocks` e `tasks`; `section_range`/`section_at`/`section_source` recortam seções; `count_tasks`/`section_tasks` contam as tarefas (`Rendered.tasks`) por trecho e por seção.
- `src/cli.rs`: parsing manual de argumentos → `Cli`/`Mode` (`Markdown`, `Json`, `Table { delimiter }`), subcomandos `json|csv|tsv`, modo deduzido da extensão, validação das flags.
- `src/main.rs`: escolha de modo e TUI. Modo plain é usado se `-p` ou se stdout não é TTY. `App` guarda o texto, o `Rendered`, um `Viewport` (scroll) e um `SearchState` (busca); re-renderiza (`rerender` → `render_doc`, o único ponto que escolhe Markdown/JSON/tabela) quando a largura da tela muda (`dirty`/`render_width`), o arquivo muda ou termina um download de imagem. Live reload é por polling de `mtime` no timeout do `event::poll` (`check_reload`), não por inotify; o mesmo tick drena o stdin em streaming (`--follow`, `drain_stream`) e troca para o `.md` mais novo da pasta (`--latest`, `follow_latest`). Busca (`/`, `n`/`N`) opera sobre o texto das linhas já renderizadas (`line_text`, `highlight_line`), por isso só acha ocorrências dentro de um mesmo trecho estilizado.
- `src/image.rs`: decodificação com limites (10 MiB, 8192 px), `halfblock` (imagem → linhas de `▀`) e `ImageStore`, o cache compartilhado de imagens locais, remotas e diagramas Mermaid (`get`, `get_mermaid`, `get_job`; jobs remotos rodam em thread e incrementam `version`).
- `src/net.rs`: download HTTP(S) restrito (`--remote`): resolvedor que recusa rede privada/loopback, teto de bytes, redirecionamento manual.
- `src/mermaid.rs`: executa o `mmdc` (`--mermaid`, binário configurável por `MDV_MMDC`) com timeout.
- `src/stream.rs`: leitor de stdin em thread (`Utf8Accum`) e `newest_md`.
- `src/mem.rs`: limite de memória do processo (`--memory-buffer`, `MDV_MEMORY_BUFFER`, padrão 512 MiB). `CountingAlloc` é o `#[global_allocator]` (conta bytes vivos, teto duro `limite×1,5+16 MiB`); `check()`/`exceeded()` são os pontos de checagem usados por `render_with`, `layout_table`, `render_code`, `data.rs` e `main.rs`; ao estourar devolvem `error_rendered(MemoryLimit)`. `check_parse` barra antes de analisar o texto cuja árvore do `pulldown-cmark` passaria do limite. Testes unitários **não** alteram o limite global (rodam em paralelo); o estouro real é testado pelo binário em `tests/cli.rs`.
- `src/clip.rs`: base64 e OSC 52 (`y`/`Y`); a TUI guarda a sequência em `App.pending_osc` e o `event_loop` a escreve.
- `src/extract.rs`: `--extract-code`/`--section`; trabalha sobre o fonte com `pulldown-cmark` (`render::parse_options`), sem renderizar, e devolve texto saneado.
- `src/data.rs`: visualizadores de dados: JSON/JSONL (`render_json` com as vistas texto, tabela e esquema, escolhidas por `--as`/tecla `v`) e CSV/TSV (`render_table_data`); as duas tabelas passam por `finish_table` (tetos `MAX_TABLE_COLS`/`MAX_CELL_CHARS`), reaproveitando `render_code_only` e `render_table_only`.

**Regra: I/O fica fora de `render.rs`.** Disco, rede e subprocesso vivem em `ImageStore`/`net`/`mermaid`; `render` só consulta o cache. Rede e subprocesso são opt-in (`--remote`, `--mermaid`).

Qualquer recurso novo de formatação entra em `render.rs` e vale automaticamente para TUI e `--plain`.

Limitações conhecidas (do README): código largo é quebrado (sem scroll horizontal), células de tabela largas são truncadas com `…`, imagens usam meio-bloco (baixa resolução; SVG e `data:` URIs não são suportados), imagem dentro de célula de tabela fica como texto alternativo.
