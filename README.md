# mdv

Visualizador de Markdown no terminal, em Rust.

```bash
cargo run --release -- sample.md     # modo interativo (TUI)
cargo run --release -- -p sample.md  # só imprime, como um `cat` colorido
cat README.md | cargo run --release  # lê da entrada padrão
```

## Teclas

| Tecla            | Ação                          |
|------------------|-------------------------------|
| `j` / `k`, ↓ / ↑ | rola uma linha                |
| espaço / `b`     | página abaixo / acima         |
| `d` / `u`        | meia página                   |
| `g` / `G`        | início / fim                  |
| `]` / `[`        | próximo / título anterior     |
| `/`              | buscar (Enter confirma)       |
| `n` / `N`        | próxima / anterior ocorrência |
| `r`              | recarrega o arquivo           |
| `q`, `Esc`       | sai                           |

O arquivo é recarregado sozinho quando muda no disco (live reload).

## Estrutura

- `src/render.rs`: Markdown → `Vec<Line>` do ratatui, já quebrado na largura pedida
  (listas aninhadas, citações, tabelas com alinhamento, código com `syntect`).
  Também converte para ANSI para o modo `--plain`.
- `src/main.rs`: argumentos, TUI (scroll, busca, índice de títulos) e live reload.

## Limitações conhecidas

- Linhas de código largas são quebradas na largura da tela, não rolam na horizontal.
- Células de tabela largas são truncadas com `…` (não quebram em várias linhas).
- A busca destaca ocorrências dentro de um mesmo trecho estilizado.
- Imagens aparecem como `[imagem: texto alternativo]`.

## Próximos passos

- Temas (claro/escuro) e escolha do tema do `syntect`.
- Imagens via protocolo Kitty/Sixel (`ratatui-image`).
- Abrir links com `xdg-open` e seguir links entre arquivos `.md`.
- Índice lateral de títulos.
