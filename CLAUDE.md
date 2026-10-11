# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Comandos

```bash
cargo build --release
cargo run --release -- sample.md     # TUI interativa
cargo run --release -- -p sample.md  # modo --plain (imprime ANSI, sem TUI)
cargo run --release -- -p -w 60 sample.md  # largura fixa
cat README.md | cargo run --release  # stdin
cargo clippy
```

Não há testes automatizados (nenhum `#[cfg(test)]` nem `tests/`). Para validar mudanças de renderização, use `-p` com `sample.md` — funciona sem terminal interativo.

O projeto, README e mensagens de erro/ajuda são em português.

## Arquitetura

Dois arquivos, um único pipeline: Markdown → `Vec<Line>` do ratatui → (TUI | ANSI).

- `src/render.rs`: `render(md, width) -> Rendered { lines, headings }`. Máquina de estados sobre eventos do `pulldown-cmark` (`Renderer::handle/start/end`). As linhas saem **já quebradas na largura pedida** (quebra com `unicode-width`, em `wrap`/`fit`/`split_by_width`), então quem consome não faz wrap. `headings` guarda o índice em `lines` de cada título (usado na navegação `]`/`[`). Blocos de código usam `syntect` (`default-fancy`, sem onig; syntax/theme em `OnceLock` via `hl()`). `to_ansi` converte as mesmas linhas para SGR no modo `--plain`.
- `src/main.rs`: parsing manual de argumentos, escolha de modo e TUI. Modo plain é usado se `-p` ou se stdout não é TTY. `App` guarda o markdown, o `Rendered` e o estado de scroll/busca; re-renderiza (`rerender`) quando a largura da tela muda (`dirty`/`render_width`) ou o arquivo muda. Live reload é por polling de `mtime` no timeout de 250 ms do `event::poll` (`check_reload`), não por inotify. Busca (`/`, `n`/`N`) opera sobre o texto das linhas já renderizadas (`line_text`, `highlight_line`), por isso só acha ocorrências dentro de um mesmo trecho estilizado.

Qualquer recurso novo de formatação entra em `render.rs` e vale automaticamente para TUI e `--plain`.

Limitações conhecidas (do README): código largo é quebrado (sem scroll horizontal), células de tabela largas são truncadas com `…`, imagens viram `[imagem: alt]`.
