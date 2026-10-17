# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Comandos

```bash
cargo build --release                 # LTO thin (rebuild mais rápido)
cargo build --profile dist            # distribuição: LTO fat, codegen-units = 1 (target/dist/)
cargo run --profile fast -- sample.md  # sem LTO: recompila bem mais rápido para testar à mão
cargo run --release -- sample.md     # TUI interativa
cargo run --release -- -p sample.md  # modo --plain (imprime ANSI, sem TUI)
cargo run --release -- -p -w 60 sample.md  # largura fixa
cat README.md | cargo run --release  # stdin
cargo run --release -- -p samples/llm.md       # front matter, alertas, <details>...
cargo run --release -- -p samples/img.md       # imagem local em meio-bloco
cargo run --release -- -p csv samples/dados.csv   # também: json, tsv
cargo run --release -- -p json samples/itens.json   # tabela; --as schema para o esquema
cargo run --release -- -p sample.md              # diagramas Mermaid só viram imagem com --graphics (terminal Kitty); sem isso saem como código
cargo run --release -- samples/tabelas-export.md -o /tmp/t.xlsx   # exporta (.pdf .docx .odt .xlsx .ods)
cargo run --release -p mdv-tools -- check-links sample.md   # binário mdv-tools
cargo clippy --workspace --all-targets
cargo test --workspace --no-default-features   # sem Mermaid nem rede (features `mermaid`, `remote`)
```

`cargo test --workspace` roda os testes unitários (`render/`, `main.rs`, `cli.rs`, `image.rs`, `net.rs`, `mermaid_native.rs`, `stream.rs`, `data.rs`, `golden_ansi.rs`, `app/keys_tests.rs`, mais os de `crates/mdv-text`, `crates/mdv-tools` e `crates/mdv-export`), `tests/cli.rs` (executa o binário) e `tests/golden.rs` (saída ANSI contra `tests/golden/`, sem `UPDATE_GOLDEN=1`). Para validar mudanças de renderização, também use `-p` com `sample.md` ou com `samples/` — funciona sem terminal interativo.

O projeto, README e mensagens de erro/ajuda são em português.

## Memória: limitar a aplicação, nunca estourar a máquina

Nenhum teste (nem execução manual) pode alocar memória sem teto. Entrada hostil ou gigante se resolve **limitando o uso de memória do `mdv`** (tetos de bytes, de itens, de caracteres por chave/célula, de profundidade), nunca rodando para ver se aguenta.

- Antes de escrever ou rodar um teste com entrada grande, calcule o pior caso de memória (inclusive o que o código derivado gera: concatenar o prefixo de uma chave de N caracteres para cada um de M filhos custa N×M). Se passar de algumas dezenas de MiB, **não rode**: corrija o limite no código primeiro e só então teste com tamanhos pequenos que provem o limite (por exemplo, teto reduzido por constante ou parâmetro de teste).
- Entradas de teste de estresse ficam em dezenas de MiB no máximo; prefira provar o teto com um valor um pouco acima dele.
- Se a ferramenta for rodar algo potencialmente pesado, use `ulimit -v` (por exemplo `(ulimit -v 2000000; cargo test ...)`) para o processo morrer sozinho em vez de travar o terminal.
- O `mdv` já tem esse fail-safe (`src/mem.rs`): novo laço que cresça com a entrada deve chamar `mem::check()`/`mem::exceeded()`.

## Arquitetura

Um único pipeline: entrada (Markdown, JSON, CSV/TSV) → `Vec<Line>` do ratatui → (TUI | ANSI).

- `src/render/` (módulos: `mod.rs` com a API pública, `model`, `outline`, `theme`, `ansi`, `wrap`, `inline`, `list`, `table`, `highlight`, `images`, `renderer`; testes em `tests.rs`/`testutil.rs`; nenhum arquivo de produção passa de 500 linhas, verificado por teste): `render_with(md, &RenderOpts { width, images }) -> Rendered { lines, headings, outline }` (`render(md, width)` é o atalho sem imagens). Máquina de estados sobre eventos do `pulldown-cmark` (`Renderer::handle/start/end`), com o estado dividido em `InlineBuffer` (texto em linha, estilos, links), `ListStack`, `CodeBlock` e `TableBuilder`. As linhas saem **já quebradas na largura pedida** (quebra com `unicode-width`, em `wrap`/`fit`/`split_by_width`), então quem consome não faz wrap. `headings` guarda o índice em `lines` de cada título (navegação `]`/`[`); `outline` guarda nível e texto (`--toc`). Blocos de código usam `syntect` (sem onig; syntax/theme em `OnceLock` via `hl()`). `to_ansi` converte as mesmas linhas para SGR no modo `--plain`. Também entende front matter YAML, alertas GFM, notas de rodapé, `<details>`/`<summary>`. Pontos de entrada extras: `render_code_only` (fonte crua, tecla `m`), `render_table_only` (CSV/TSV) e `error_rendered`. Todo texto vindo do documento passa por `sanitize` (caracteres de controle viram U+FFFD). `Rendered` também guarda `line_src` (linha do fonte de cada linha renderizada, vazio fora do Markdown), `code_blocks` e `tasks`; `section_range`/`section_at`/`section_source` recortam seções; `count_tasks`/`section_tasks` contam as tarefas (`Rendered.tasks`) por trecho e por seção. `code_overflow` (limite da rolagem horizontal de código). Imagens em célula de tabela viram figuras numeradas depois da tabela (`flush_table_figures`).
- `src/cli.rs`: parsing manual de argumentos → `Cli { common, command }`. `Common` (caminho, `-p`, `-w`, `--memory-buffer`, `--graphics`, `--remote`, `--follow`, `--table`) vale para todo formato; `Command` é `View(Doc)`, `Toc`, `Stats`, `Help` ou `Version`, e `Doc` é `Markdown | Json { json_path, view } | Table { delimiter, head, cols, no_header }`, de modo que cada opção só existe na variante a que pertence. Uma função de parsing por formato (`parse_markdown`/`parse_json`/`parse_table`); subcomandos `json|csv|tsv`, formato deduzido da extensão (`--delimiter` força tabela). Opção fora do seu formato é erro de uso (`a opção --head só vale para CSV/TSV`); `--toc`/`--stats` só valem para Markdown.
- `src/main.rs` (~120 linhas): `mod`s, `#[global_allocator]`, `Failure` e `run_cli` (escolha de modo: plain, `--follow`, seletor de pasta ou TUI). Modo plain é usado se `-p` ou se stdout não é TTY. `src/load.rs` lê a entrada (`Source`, teto de 16 MiB, `image_store`); `src/plain.rs` tem `run_plain` e o `HELP`; `src/doc.rs` tem `DocOpts`, `render_doc` (o único ponto que escolhe Markdown/JSON/tabela) e `Document` (texto, `Rendered`, `render_width`, `dirty`; `render`/`needs_render`).
- `src/app/`: o estado de tela da TUI. `App` (`mod.rs`) guarda um `Document`, o `Viewport` (`viewport.rs`, scroll indexa `doc.rendered.lines`), o `SearchState` (`search.rs`) e um `Modal` (`modal.rs`: `None`, `Search`, `TableZoom`, `ImageZoom`, `Picker`, `ConfirmOpen`; só um por vez, e `Ctrl-C` sai em todos, tratado no início de `on_key`). As teclas do modo normal viram `Action` (`input.rs`: `action_for` é uma função pura, `App::apply` executa); `ui.rs` desenha (`draw`, barra de status), `zoom.rs` guarda o zoom de tabela, `terminal.rs` o laço de eventos. `App::rerender` re-renderiza (`Document::render`) quando a largura da tela muda (`needs_render`), o arquivo é recarregado (tecla `r`, `reload`) ou termina um download de imagem. Não há recarga automática; o tick do `event::poll` drena o stdin em streaming (`--follow`, `drain_stream`). Busca (`/`, `n`/`N`) opera sobre o texto das linhas já renderizadas (`line_text`, `highlight_line`), por isso acha ocorrências dentro de uma mesma linha renderizada (não atravessa a quebra de linha). Nenhum arquivo de produção passa de 500 linhas (teste em `app/tests.rs`).
- `src/image.rs`: decodificação com limites (10 MiB, 8192 px), `halfblock` (imagem → linhas de `▀`) e `ImageStore`, o cache compartilhado de imagens locais, remotas e diagramas Mermaid (`get`, `get_mermaid`, `get_job`; jobs remotos rodam em thread e incrementam `version`).
- `src/graphics.rs`: protocolo gráfico do Kitty por placeholders Unicode (`cell_size`, `placeholder_rows`, `transmit`, `supports_kitty`); só funções puras. `ImageStore::graphic` dá o `id`/PNG (cache limitado por `MAX_GRAPHICS`), `render.rs` desenha os placeholders e registra `Rendered.graphics`, e `App::prepare` renderiza antes do quadro e escreve `pending_gfx` no terminal (`--graphics`, só no modo interativo).
- `src/net.rs`: download HTTP(S) restrito (`--remote`): resolvedor que recusa rede privada/loopback, teto de bytes, redirecionamento manual.
- `src/imgzoom.rs` e `src/imgview.rs`: zoom de imagem (tecla `i`). `imgzoom` tem só contas puras (`fit_scale`, `max_scale`, `frame`, `View`); `imgview` guarda `ImgZoom` (decodifica sob demanda o PNG original, `ImageStore::mermaid_png`, que o cache reduzido não guarda) e o `impl App` (`open_visible_mermaid`, `prepare_img_zoom` antes do quadro, `draw_img_zoom`). No Kitty reenvia o recorte com o `id` fixo `ZOOM_ID` (blocos limitados a `graphics::MAX_CELLS`); fora dele (só se o cache gráfico estourar `MAX_GRAPHICS`, pois o Mermaid exige gráficos) usa meio-bloco. `Rendered.mermaids` (`MermaidSpan`) liga linhas renderizadas ao PNG.
- `src/mermaid_native.rs`: Mermaid → SVG (`mermaid-rs-renderer`) → PNG (`resvg`) em memória; tetos (`MAX_LINES` 400 linhas, `MAX_CONNECTIONS` 100 conexões contadas por `>` e `&`, `MAX_SVG_BYTES`, 1200×3000 px), erro claro sem fontes do sistema (`font_count`) e `guarded` (pânico vira erro; o gancho é instalado uma vez e fica mudo só na thread que renderiza, via `thread_local`, para não sujar a TUI sem esconder pânicos de outras threads).
- `src/config.rs`: arquivo de configuração do usuário (`$XDG_CONFIG_HOME/mdv/config`, chave `mermaid = on|off`, teto de 64 KiB); `load::image_store` o consulta e `--graphics` tem padrão `auto` (Kitty/Ghostty ligam gráficos e Mermaid sozinhos).
- `src/stream.rs`: leitor de stdin em thread (`Utf8Accum`, `spawn_stdin_reader`).
- `crates/mdv-text` (lib): `sanitize`, `parse_options`, `slugify`, `percent_decode`, `is_external`, `is_confined`, `Heading`, `headings`, `section_text`; sem I/O, só `pulldown-cmark`. `render.rs` reexporta `sanitize` e `parse_options` (`pub(crate) use`).
- `crates/mdv-tools` (lib + bin `mdv-tools`): `extract-code`, `section`, `check-links` (antigos `--extract-code`, `--section`, `--check-links`); `extract.rs` e `links.rs` com os tetos antigos (200 problemas, 20 000 links, 50 `.md` de 5 MiB), `input.rs` com o teto de 16 MiB; depende só de `mdv-text`, nunca do `mdv`. O pacote `mdv` fica na raiz; as duas crates ficam em `crates/`.
- `crates/mdv-export` (lib): `convert(&Input, &Select, Format, &Context) -> Result<Output, String>` (Markdown → PDF/DOCX/ODT; Markdown/CSV/TSV → XLSX/ODS); depende só de `mdv-text`; o limite de memória (`Budget`) e as imagens (`PictureProvider`) entram por parâmetro e o `src/export.rs` do `mdv` os adapta a `mem` e `ImageStore`; escritores à mão (ZIP, XML, PDF), sem I/O de arquivo; `src/export.rs` = gravação atômica + `run_export` (`-o`).
- `src/mem.rs`: limite de memória do processo (`--memory-buffer`, `MDV_MEMORY_BUFFER`, padrão 512 MiB). `CountingAlloc` é o `#[global_allocator]` (conta bytes vivos, teto duro `limite×1,5+16 MiB`); `check()`/`exceeded()` são os pontos de checagem usados por `render_with`, `layout_table`, `render_code`, `data.rs` e `main.rs`; ao estourar devolvem `error_rendered(MemoryLimit)`. `check_parse` barra antes de analisar o texto cuja árvore do `pulldown-cmark` passaria do limite. Testes unitários **não** alteram o limite global (rodam em paralelo); o estouro real é testado pelo binário em `tests/cli.rs`.
- `src/clip.rs`: base64 e OSC 52 (`y`/`Y`); a TUI guarda a sequência em `App.pending_osc` e o `event_loop` a escreve.
- `src/nav.rs` e `src/browse.rs`: navegação na TUI. `nav::resolve` classifica o destino de um link (âncora, `.md` relativo confinado, `http(s)`/`mailto` sem `?attach=`); `open_external` roda `xdg-open`/`open` só depois do `y`. O texto de cada link carrega o índice em `Rendered.links` na cor do sublinhado (`render::link_id`; `strip_link_marks` tira a marca antes de desenhar). `browse.rs` tem `Tab`/`Enter`/`Backspace`, a pilha `App.back` e `open_file`. `open_file` recebe o `Doc` do documento aberto (links seguem como Markdown) e `Back::File` guarda o `Doc` para o `Backspace` restaurar. Textos vindos de URL, âncora e nome de arquivo passam por `render::sanitize` antes de ir para a barra ou o seletor.
- `src/picker.rs`: seletor de `mdv <pasta>` (não recursivo, teto de 5000 arquivos, filtro fuzzy). `App.picker` ocupa a tela enquanto existe.
- `src/data.rs`: visualizadores de dados: JSON/JSONL (`render_json` com as vistas texto, tabela e esquema, escolhidas por `--as`/tecla `v`) e CSV/TSV (`render_table_data`); as duas tabelas passam por `finish_table` (tetos `MAX_TABLE_COLS`/`MAX_CELL_CHARS`), reaproveitando `render_code_only` e `render_table_only`.

**Regra: `#[cfg(feature)]` só nas fachadas** (`mermaid_native.rs` e `net.rs`, cada um com uma versão vazia que devolve erro claro); testes que usam Mermaid ou rede levam `#[cfg(feature = "…")]`. `images` é sempre compilada (decisão da fase 6). Não rode `cargo fmt` (o repositório não é rustfmt-clean).

**Regra: I/O fica fora de `render/`.** Disco, rede e subprocesso vivem em `ImageStore`/`net`/`mermaid_native`; `render` só consulta o cache, por meio do `trait ImageProvider` (`render/images.rs`; o `ImageStore` o implementa e os testes usam um fake). `mermaid_enabled()` só é verdadeiro com gráficos ativos: sem eles o bloco ` ```mermaid ` sai como código. A rede é opt-in (`--remote`); o Mermaid roda em processo (sem subprocesso).

Qualquer recurso novo de formatação entra em `render/` e vale automaticamente para TUI e `--plain`.

Limitações conhecidas (do README): código largo é quebrado por padrão (tecla `w` liga rolagem horizontal), células de tabela largas são truncadas com `…`, imagens usam meio-bloco (baixa resolução; SVG não é suportado).
