# mdv

**Leia Markdown no terminal como deveria ser: com cor, estrutura e sem sair do teclado.**

`mdv` transforma qualquer arquivo `.md` numa página bem formatada direto no terminal — títulos em destaque, tabelas alinhadas, listas aninhadas, código colorido — e abre num leitor interativo com busca, navegação por títulos e recarga automática. Um único binário, sem runtime, sem navegador.

[![release](https://img.shields.io/github/v/release/flisbao/mdv)](https://github.com/flisbao/mdv/releases/latest)
[![ci](https://github.com/flisbao/mdv/actions/workflows/ci.yml/badge.svg)](https://github.com/flisbao/mdv/actions/workflows/ci.yml)

Além de Markdown, abre **JSON, CSV e TSV** como dados formatados, desenha **imagens e diagramas Mermaid** no próprio terminal e acompanha a saída de **agentes e LLMs** em tempo real.

```bash
mdv README.md                 # leitor interativo
mdv -p README.md              # só imprime, como um `cat` colorido
curl -s https://exemplo/doc.md | mdv    # lê da entrada padrão
agente | mdv --follow         # acompanha um texto que ainda está chegando
mdv csv vendas.csv --cols produto,total   # CSV como tabela
mdv json dados.json --path .itens[0]      # só um trecho do JSON
```

## Por que usar

- **Renderização completa.** Títulos, ênfase, ~~tachado~~, `código inline`, links, citações, listas ordenadas, não ordenadas, aninhadas e de tarefas (`- [x]`), réguas e tabelas com alinhamento por coluna.
- **Código com realce de sintaxe.** Blocos com linguagem declarada saem coloridos (via `syntect`), com moldura e rótulo da linguagem.
- **Tabelas que cabem na tela.** Colunas se ajustam à largura disponível; células largas são truncadas com `…` em vez de quebrar o layout.
- **Quebra de linha correta.** O texto é quebrado na largura do terminal respeitando caracteres largos: acentos, emojis e CJK (日本語) não desalinham nada.
- **Leitor interativo.** Rolagem, página, meia página, busca com `/`, `n`/`N` entre ocorrências e `]`/`[` para pular de título em título.
- **Imagens no terminal.** `![alt](arquivo.png)` e, com `--remote`, imagens por URL, desenhadas em meio-bloco, sem protocolo gráfico. [Detalhes](#imagens).
- **Diagramas Mermaid.** Com `--mermaid`, blocos <code>```mermaid</code> viram imagem via `mmdc`. [Detalhes](#mermaid).
- **Feito para saída de agentes e LLMs.** Front matter YAML, alertas GFM, notas de rodapé, `<details>` e `<think>` aparecem formatados; `--follow`, `--latest <pasta>` e `--toc` acompanham e resumem. Na TUI, `m` alterna para a fonte crua. [Detalhes](#markdown-de-agentes-e-llms).
- **JSON, CSV e TSV.** `mdv json|csv|tsv` com `--path`, `--head`, `--cols`, `--no-header` e `--delimiter`. [Detalhes](#json-csv-e-tsv).
- **Live reload.** Edite o arquivo no seu editor e veja o resultado atualizar sozinho no `mdv` ao lado. Ótimo para escrever README.
- **Funciona em pipe.** Com `-p` ou quando a saída não é um terminal, imprime texto com ANSI. Serve em scripts, `less -R` e CI.
- **Responsivo.** Redesenha ao redimensionar a janela; `-w` fixa a largura máxima.
- **Seguro com entrada não confiável.** Caracteres de controle e sequências de escape do documento são neutralizados, então um `.md` malicioso não consegue mexer no seu terminal. Tamanho de entrada é limitado, e rede e subprocessos só entram com `--remote` e `--mermaid`. Veja as [pendências conhecidas](docs/pendencias.md).
- **Rápido e portátil.** Binário único em Rust para Linux (glibc e estático musl), macOS e Windows.

## Instalação

Baixe o binário da última release, extraia e coloque `mdv` no seu `PATH`:

<!-- mdv:install:start -->
| Plataforma | Download |
|------------|----------|
| Linux x86_64 (glibc 2.35+) | [`mdv-v0.2.0-linux-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.2.0/mdv-v0.2.0-linux-x86_64.tar.gz) |
| Linux x86_64 (estático, qualquer distro, Alpine inclusive) | [`mdv-v0.2.0-linux-x86_64-musl.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.2.0/mdv-v0.2.0-linux-x86_64-musl.tar.gz) |
| Linux aarch64 | [`mdv-v0.2.0-linux-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.2.0/mdv-v0.2.0-linux-aarch64.tar.gz) |
| macOS Apple Silicon | [`mdv-v0.2.0-macos-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.2.0/mdv-v0.2.0-macos-aarch64.tar.gz) |
| macOS Intel | [`mdv-v0.2.0-macos-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.2.0/mdv-v0.2.0-macos-x86_64.tar.gz) |
| Windows x86_64 | [`mdv-v0.2.0-windows-x86_64.zip`](https://github.com/flisbao/mdv/releases/download/v0.2.0/mdv-v0.2.0-windows-x86_64.zip) |
| Debian/Ubuntu e Fedora/RHEL | pacotes `.deb` e `.rpm` na [página da release](https://github.com/flisbao/mdv/releases/tag/v0.2.0) |

Versão atual: **v0.2.0**. Cada release traz um `SHA256SUMS` para conferir os arquivos.
<!-- mdv:install:end -->

Os binários de macOS e Windows **não são assinados**, então o sistema pode avisar na primeira execução:

- **macOS (Gatekeeper):** se o binário baixado pelo navegador for bloqueado, remova a quarentena com `xattr -d com.apple.quarantine mdv`.
- **Windows (SmartScreen):** se aparecer o aviso de aplicativo desconhecido, escolha "Mais informações" e depois "Executar assim mesmo". Use o Windows Terminal para ver cores e a TUI corretamente.

Ou compile do código-fonte (Rust estável):

```bash
cargo install --git https://github.com/flisbao/mdv
```

## Uso

<!-- mdv:help:start -->
```text
mdv - visualizador de Markdown no terminal

USO:
    mdv [opções] <arquivo.md>
    cat arquivo.md | mdv
    mdv json [opções] <arquivo|->
    mdv csv|tsv [opções] <arquivo|->
    (o formato também é deduzido da extensão: .json, .jsonl, .csv, .tsv)

OPÇÕES:
    -p, --plain        imprime no terminal (sem TUI), como `cat` com cores
    -w, --width <N>    largura máxima do texto (padrão: 100)
        --remote       baixa imagens de http(s):// (nunca de rede privada)
        --mermaid      renderiza blocos ```mermaid via `mmdc` (ou $MDV_MMDC)
        --toc          imprime só o índice de títulos
        --follow       com stdin: acompanha o texto conforme chega
        --latest       o argumento é uma pasta: abre e segue o .md mais novo
    -h, --help         mostra esta ajuda
    -V, --version      mostra a versão

OPÇÕES DE JSON:
        --path <p>     mostra só um trecho, ex.: .itens[0].nome

OPÇÕES DE CSV/TSV:
        --head <N>     mostra só as N primeiras linhas (padrão: 1000)
        --cols <a,b>   mostra só estas colunas, pelo nome do cabeçalho
        --no-header    a primeira linha é dado, não cabeçalho
        --delimiter <c>  separador de um caractere (ou `tab`)

TECLAS (modo interativo):
    j / k, ↓ / ↑       rola uma linha
    espaço / b         página abaixo / acima
    d / u              meia página
    g / G              início / fim
    ] / [              próximo / título anterior
    /                  buscar (Enter confirma, Esc cancela)
    n / N              próxima / anterior ocorrência
    m                  alterna entre renderizado e fonte crua
    r                  recarrega o arquivo
    q, Esc             sai
```
<!-- mdv:help:end -->

## Imagens

Imagens `![alt](caminho)` aparecem desenhadas no próprio terminal, em **meio-bloco** (`▀` com cor de frente e de fundo), então funcionam na TUI e no `--plain` sem protocolo gráfico. Caminhos relativos são resolvidos a partir da pasta do arquivo (com stdin, do diretório atual).

- **Locais:** PNG, JPEG, GIF e WebP, até 10 MiB e 8192×8192 px. Se não abrir, aparece `[imagem: alt — motivo]`.
- **Por URL:** só com `--remote`. Sem a flag, o `mdv` nunca acessa a rede. O download recusa endereços de rede privada, loopback e link-local, limita o tamanho a 10 MiB, segue no máximo 3 redirecionamentos e aceita até 32 imagens por documento. Na TUI o texto aparece na hora e a imagem entra quando o download termina.
- **Não suportados:** SVG e `data:` URIs. A resolução é baixa (meio-bloco); imagem dentro de célula de tabela fica como texto alternativo.

## Mermaid

Com `--mermaid`, blocos <code>```mermaid</code> são renderizados pelo [`mmdc`](https://github.com/mermaid-js/mermaid-cli) (`npm i -g @mermaid-js/mermaid-cli`) e exibidos como imagem em meio-bloco. O binário pode ser trocado com a variável `MDV_MMDC`. Há limite de 20 s por diagrama e de 64 KiB de código. Sem a flag, ou sem o `mmdc`, o bloco continua aparecendo como código, com o motivo no cabeçalho.

> O `mmdc` abre um Chromium headless: use `--mermaid` só com documentos em que você confia.

## Markdown de agentes e LLMs

Construções comuns em saídas de LLMs aparecem formatadas, sem tags cruas: front matter YAML (como bloco `yaml`), alertas GFM (`> [!WARNING]`), notas de rodapé, `<details>`/`<summary>` e blocos `<think>`/`<thinking>` (em itálico apagado). Para acompanhar uma saída em andamento:

```bash
agente | mdv --follow          # o texto cresce ao vivo e a tela acompanha o fim
mdv --latest ./respostas       # abre e segue o .md mais novo da pasta
mdv --toc relatorio.md         # só o índice de títulos
```

Na TUI, a tecla `m` alterna entre o texto renderizado e a **fonte crua**.

## JSON, CSV e TSV

```bash
mdv json dados.json --path .itens[0]   # só um trecho; JSONL vira lista
mdv csv vendas.csv --head 20 --cols produto,total
mdv tsv planilha.tsv --no-header
mdv csv dados.txt --delimiter ';'
```

O formato também é deduzido da extensão (`.json`, `.jsonl`, `.csv`, `.tsv`) e funciona com `-` (stdin). O JSON sai indentado, com realce e um resumo de tipo e tamanho. CSV/TSV viram tabela com colunas numéricas alinhadas à direita e um rodapé `N de M linhas · K colunas`; use `--cols` para tabelas largas, pois não há rolagem horizontal. A entrada de dados é limitada a 5 MiB.

## Limitações conhecidas

- Linhas de código largas são quebradas na largura da tela, não rolam na horizontal.
- Células de tabela largas são truncadas com `…`.
- A busca encontra ocorrências dentro de um mesmo trecho estilizado.
- Imagens usam meio-bloco (baixa resolução); um protocolo gráfico como Kitty/Sixel seria um refinamento opcional.

## Changelog

<!-- mdv:changelog:start -->
### v0.2.0

_2026-10-13_

- test(net): fixar que grafias alternativas de loopback são bloqueadas
- fix(seguranca): teto na saída do JSON indentado e não abrir FIFOs/dispositivos como imagem
- fix(seguranca): prazo de conexão no download e filtro SSRF para NAT64, 6to4 e faixas reservadas
- fix(seguranca): sanitizar rótulos de nota e erros de imagem, limitar leitura/cache de imagens e mmdc
- docs: imagens, mermaid, markdown de LLMs e modos JSON/CSV/TSV
- feat(dados): mdv csv/tsv com --head, --cols, --no-header e alinhamento numérico
- feat(dados): mdv json com --path, suporte a JSONL e saneamento
- test(cli): ignorar BrokenPipe ao escrever stdin quando o mdv sai antes de ler
- feat: --follow para stdin em streaming e --latest para acompanhar a pasta
- feat: tecla m (fonte crua) e --toc
- feat(render): front matter, alertas GFM, notas de rodapé, details e think de LLMs
- feat(mermaid): renderiza blocos mermaid via mmdc com --mermaid
- feat(imagens): imagens por URL com --remote, política anti-SSRF e download em segundo plano
- feat(imagens): renderização de imagens locais em meio-bloco
- feat(cli): subcomandos json/csv/tsv e flags novas com validação
- feat(release): acrescentar changelog ao README a cada release

### v0.1.1

_2026-10-13_

- ci/docs: release multiplataforma e README atualizado a cada release (#1)
- chore: track Cargo.lock for reproducible release builds
<!-- mdv:changelog:end -->

## Desenvolvimento

```bash
cargo run --release -- sample.md        # TUI
cargo run --release -- -p -w 60 sample.md   # saída plain
cargo test
```

O pipeline é um só: entrada (Markdown, JSON, CSV/TSV) → linhas do `ratatui` → TUI ou ANSI. `src/render.rs` renderiza; `src/main.rs` cuida da TUI, busca e live reload; `src/cli.rs` interpreta os argumentos; `src/image.rs`, `src/net.rs` e `src/mermaid.rs` fazem o I/O de imagens e diagramas (nunca dentro de `render.rs`); `src/stream.rs` trata `--follow` e `--latest`; `src/data.rs` trata JSON e CSV/TSV.

Os blocos de **instalação** e **uso** acima são gerados pela CI a cada release (`scripts/update-readme.sh`), a partir dos arquivos publicados e da saída real de `mdv --help`; não edite o que está entre os marcadores `<!-- mdv:… -->`.

## Licença

MIT
