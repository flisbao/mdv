# Tabelas

## Tabela simples

| Linguagem | Ano | Tipagem    |
|-----------|-----|------------|
| Rust      | 2015 | estática  |
| Python    | 1991 | dinâmica  |
| Go        | 2009 | estática  |

## Alinhamento

| Esquerda | Centro | Direita |
|:---------|:------:|--------:|
| a        |   b    |       c |
| texto mais longo | meio | 1234,56 |

## Tabela com muitas colunas

| Coluna 1 | Coluna 2 | Coluna 3 | Coluna 4 | Coluna 5 | Coluna 6 | Coluna 7 | Coluna 8 | Coluna 9 | Coluna 10 |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| um valor bem maior que os outros para forçar o ajuste de largura | b | c | d | e | f | g | h | i | j |

## Célula com código e formatação

| Comando | Descrição |
|---|---|
| `cargo build` | compila o **projeto** |
| `cargo test` | roda os *testes* |

## Tabela com caracteres largos

| Emoji | Nome |
|---|---|
| 🦀 | Rust |
| 🐍 | Python |
| 日本語 | texto em japonês |

## Célula longa

| Arquivo | Descrição |
|---|---|
| `src/render.rs` | Máquina de estados sobre os eventos do pulldown-cmark: as linhas já saem quebradas na largura pedida, então quem consome não faz wrap. |
| `https://example.com/um/caminho/muito/longo/sem/espacos/que/nao/cabe/na/coluna/index.html` | URL sem espaços quebra por caractere |
