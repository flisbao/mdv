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
- **Tabelas que cabem na tela.** Colunas se ajustam à largura disponível; células largas quebram em várias linhas (até 8, depois `…`). Na TUI, `t` abre a tabela na largura inteira do terminal, com cabeçalho e primeira coluna fixos e `h`/`l` para rolar as colunas; `--table N` abre direto numa delas.
- **Quebra de linha correta.** O texto é quebrado na largura do terminal respeitando caracteres largos: acentos, emojis e CJK (日本語) não desalinham nada.
- **Leitor interativo.** Rolagem, página, meia página, busca com `/`, `n`/`N` entre ocorrências e `]`/`[` para pular de título em título.
- **Imagens no terminal.** `![alt](arquivo.png)` e, com `--remote`, imagens por URL, desenhadas em meio-bloco, sem protocolo gráfico. [Detalhes](#imagens).
- **Diagramas Mermaid.** Com `--mermaid`, blocos <code>```mermaid</code> viram imagem com o renderizador embutido, sem instalar nada (o `mmdc` é a alternativa para casos complexos). [Detalhes](#mermaid).
- **Andamento de tarefas.** Listas `- [x]`/`- [ ]` mostram o progresso: o rodapé da leitura traz `☑ feitas/total` do documento e da seção atual, e o `--toc` acrescenta `[feitas/total]` aos títulos. Sem tarefas no documento (e no modo fonte, JSON e CSV) nada disso aparece.
- **Feito para saída de agentes e LLMs.** Front matter YAML, alertas GFM, notas de rodapé, `<details>` e `<think>` aparecem formatados; `--follow`, `--latest <pasta>` e `--toc` acompanham e resumem. Na TUI, `m` alterna para a fonte crua. [Detalhes](#markdown-de-agentes-e-llms).
- **JSON, CSV e TSV.** `mdv json|csv|tsv` com `--as table|schema|text`, `--path`, `--head`, `--cols`, `--no-header` e `--delimiter`. [Detalhes](#json-csv-e-tsv).
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
| Linux x86_64 (glibc 2.35+) | [`mdv-v0.3.0-linux-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.3.0/mdv-v0.3.0-linux-x86_64.tar.gz) |
| Linux x86_64 (estático, qualquer distro, Alpine inclusive) | [`mdv-v0.3.0-linux-x86_64-musl.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.3.0/mdv-v0.3.0-linux-x86_64-musl.tar.gz) |
| Linux aarch64 | [`mdv-v0.3.0-linux-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.3.0/mdv-v0.3.0-linux-aarch64.tar.gz) |
| macOS Apple Silicon | [`mdv-v0.3.0-macos-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.3.0/mdv-v0.3.0-macos-aarch64.tar.gz) |
| macOS Intel | [`mdv-v0.3.0-macos-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.3.0/mdv-v0.3.0-macos-x86_64.tar.gz) |
| Windows x86_64 | [`mdv-v0.3.0-windows-x86_64.zip`](https://github.com/flisbao/mdv/releases/download/v0.3.0/mdv-v0.3.0-windows-x86_64.zip) |
| Debian/Ubuntu e Fedora/RHEL | pacotes `.deb` e `.rpm` na [página da release](https://github.com/flisbao/mdv/releases/tag/v0.3.0) |

Versão atual: **v0.3.0**. Cada release traz um `SHA256SUMS` para conferir os arquivos.
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
        --mermaid      renderiza blocos ```mermaid (renderizador embutido; `mmdc` ou $MDV_MMDC se precisar)
        --setup-mermaid  verifica o ambiente dos diagramas e mostra como instalar o mmdc
        --toc          imprime só o índice de títulos
        --stats        imprime palavras, blocos de código e tokens estimados, no total e por seção
        --follow       com stdin: acompanha o texto conforme chega
        --latest       o argumento é uma pasta: abre e segue o .md mais novo
        --table <N>    abre direto no zoom da N-ésima tabela (só no modo interativo)
        --memory-buffer <T>  limite de memória (padrão: 512M; ex.: 256M, 2G; ou $MDV_MEMORY_BUFFER)
        --extract-code imprime só o texto dos blocos de código (sem cor)
        --lang <L>     com --extract-code: só os blocos da linguagem L
    -n <N>             com --extract-code: só o N-ésimo bloco (a partir de 1)
        --section <T>  imprime só o Markdown da seção de título T
    -h, --help         mostra esta ajuda
    -V, --version      mostra a versão

OPÇÕES DE JSON:
        --path <p>     mostra só um trecho, ex.: .itens[0].nome
        --as <vista>   auto (padrão), text, table ou schema

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
    v                  (JSON) alterna tabela / esquema / texto
    t                  zoom da tabela na tela (h/l colunas, Esc volta)
    } / {              próxima / anterior mudança marcada no reload; c limpa as marcas
    z / Z              dobra ou abre a seção atual / dobra todas (ou abre todas); título dobrado mostra ▸ N linhas
    y / Y              copia o bloco de código da tela / o Markdown da seção (OSC 52, até 64 KiB)
    r                  recarrega o arquivo
    q, Esc             sai
```
<!-- mdv:help:end -->

## Imagens

Imagens `![alt](caminho)` aparecem desenhadas no próprio terminal, em **meio-bloco** (`▀` com cor de frente e de fundo), então funcionam na TUI e no `--plain` sem protocolo gráfico. Caminhos relativos são resolvidos a partir da pasta do arquivo (com stdin, do diretório atual).

- **Locais:** PNG, JPEG, GIF e WebP, até 10 MiB e 8192×8192 px. Se não abrir, aparece `[imagem: alt — motivo]`. Imagens embutidas como `data:image/...;base64,...` (mesmos limites) também funcionam, sem rede; até 32 por documento.
- **Por URL:** só com `--remote`. Sem a flag, o `mdv` nunca acessa a rede. O download recusa endereços de rede privada, loopback e link-local, limita o tamanho a 10 MiB, segue no máximo 3 redirecionamentos e aceita até 32 imagens por documento. Na TUI o texto aparece na hora e a imagem entra quando o download termina.
- **Alta resolução (opt-in):** `--graphics kitty` (ou `auto`, que detecta Kitty e Ghostty fora do `tmux`/`screen`) desenha as imagens e os diagramas Mermaid pelo protocolo gráfico do Kitty, com *placeholders Unicode*, então rolam, dobram e somem com o texto. Só vale no modo interativo: `--plain` e saída redirecionada seguem em meio-bloco. Sixel não é suportado.
- **Não suportados:** SVG e `data:` sem `;base64`. A resolução é baixa (meio-bloco); imagem dentro de célula de tabela vira `[imagem: alt ↓n]` na célula e é desenhada logo abaixo da tabela, com a legenda `↓n alt`.

## Mermaid

Com `--mermaid`, blocos <code>```mermaid</code> viram imagem **sem instalar nada**: o `mdv` traz um renderizador embutido (flowchart, sequence, class, state, ER, pie, Gantt, timeline, journey, mindmap e outros). Ele precisa de ao menos uma fonte instalada no sistema para desenhar o texto.

```bash
mdv --mermaid doc.md           # diagramas como imagem
mdv --setup-mermaid            # verifica o ambiente e mostra como instalar o mmdc
```

Se o renderizador embutido não conseguir desenhar um diagrama (muito complexo, ou sintaxe que ele não conhece), o `mdv` tenta o [`mmdc`](https://github.com/mermaid-js/mermaid-cli) (`npm install -g @mermaid-js/mermaid-cli`; binário trocável com `MDV_MMDC`), que exige Node e baixa um Chromium. `mdv --setup-mermaid` diz o que falta e o comando de instalação do seu sistema; ele **não instala nada sozinho**. Sem nenhum dos dois, ou sem a flag, o bloco continua aparecendo como código, com o motivo no cabeçalho. Limites: 64 KiB, 400 linhas e 100 conexões (setas `>` e `&`) por diagrama para o renderizador embutido, imagem de até 1200×3000 px, 20 s no `mmdc`. Diagramas acima desse teto de conexões vão direto para o `mmdc`, se estiver instalado.

Onde o diagrama aparece:

| Terminal | Sistemas | Como aparece |
|---|---|---|
| kitty, Ghostty | Linux, macOS | alta resolução com `--graphics auto` (confirmado) |
| WezTerm, iTerm2, Warp, Konsole | varia | meio-bloco; `--graphics kitty` por sua conta (suporte a *placeholders* Unicode não confirmado) |
| Windows Terminal, demais terminais com cores 24 bits | Windows, Linux, macOS | meio-bloco (baixa resolução, funciona em qualquer um) |

> O `mmdc` abre um Chromium headless: use-o só com documentos em que você confia. O renderizador embutido roda dentro do próprio `mdv`, sem processos externos.

## Markdown de agentes e LLMs

Construções comuns em saídas de LLMs aparecem formatadas, sem tags cruas: front matter YAML (como bloco `yaml`), alertas GFM (`> [!WARNING]`), notas de rodapé, `<details>`/`<summary>` e blocos `<think>`/`<thinking>` (em itálico apagado). Para acompanhar uma saída em andamento:

```bash
agente | mdv --follow          # o texto cresce ao vivo e a tela acompanha o fim
mdv --latest ./respostas       # abre e segue o .md mais novo da pasta
mdv --toc relatorio.md         # só o índice de títulos
```

Na TUI, a tecla `m` alterna entre o texto renderizado e a **fonte crua**.

## Extrair trechos

Para scripts e prompts, o `mdv` imprime só o que você pede, em texto puro (sem cor):

```bash
mdv --extract-code plano.md                      # todos os blocos de código
mdv --extract-code --lang bash -n 2 plano.md     # o 2º bloco bash
mdv --section "Instalação" plano.md              # o Markdown da seção
mdv --section "Instalação" --extract-code plano.md | sh
```

`--lang` não distingue maiúsculas; `-n` conta a partir de 1, depois do filtro. O título casa por igualdade (sem distinguir caixa) ou por trecho único; se for ambíguo, o `mdv` lista os candidatos e sai com código 1, o mesmo código de quando nada é encontrado.

Para dimensionar um documento antes de colá-lo num prompt, `mdv --stats plano.md` imprime palavras, blocos de código e tokens estimados, no total e por seção (a seção pai soma as subseções; `[feitas/total]` aparece quando há tarefas). Os tokens são uma estimativa: caracteres do texto, prosa e código, divididos por 4. Só vale para Markdown e não combina com `--toc`, `--extract-code`, `--section`, `--follow` nem `--latest`.

## JSON, CSV e TSV

```bash
mdv json itens.json                    # lista de objetos (e JSONL) vira tabela
mdv json dados.json --as schema        # só chaves e tipos, sem os valores
mdv json dados.json --as text          # JSON indentado e colorido
mdv json dados.json --path .itens[0]   # só um trecho
mdv csv vendas.csv --head 20 --cols produto,total
mdv tsv planilha.tsv --no-header
mdv csv dados.txt --delimiter ';'
```

Uma lista de objetos (ou JSONL) abre como **tabela**: uma coluna por chave, objetos aninhados achatados como `endereco.cidade` e listas mostradas de forma compacta. `--head` e `--cols` valem também aqui. Os outros JSONs abrem **indentados**, com um resumo de tipo e tamanho. `--as table|schema|text` força a vista, e na TUI a tecla `v` alterna entre tabela, esquema e texto. O **esquema** mostra o tipo de cada campo (`itens: lista · 120 × objeto`, `tags?: ...` quando o campo falta em alguns itens) a partir de uma amostra dos 1000 primeiros itens.

O formato também é deduzido da extensão (`.json`, `.jsonl`, `.csv`, `.tsv`) e funciona com `-` (stdin). Tabelas mostram até 200 colunas (o rodapé avisa `+N omitidas`) e cortam células com mais de 500 caracteres; para tabelas com muitas colunas use `t` (zoom) ou `--cols`. CSV/TSV têm colunas numéricas alinhadas à direita e rodapé `N de M linhas · K colunas`. A entrada de dados é limitada a 5 MiB.

## Limite de memória

Por segurança o `mdv` usa no máximo **512 MiB** de memória. Se um arquivo precisar de mais (um Markdown muito grande ou um JSON hostil), ele para e mostra no lugar do documento uma mensagem com o limite, a memória livre do computador e o valor sugerido. Para ler arquivos maiores:

```bash
mdv --memory-buffer 2G grande.md
MDV_MEMORY_BUFFER=2G mdv grande.md      # a flag vence a variável
```

Aceita `256M`, `1G`, `512MiB`, `65536K` (múltiplos de 1024); o mínimo é 32 MiB. Se o valor passar da memória livre do computador, o `mdv` avisa e segue. A memória livre é detectada no Linux; no macOS e no Windows a mensagem só não traz o número.

## Limitações conhecidas

- Linhas de código largas são quebradas na largura da tela; a tecla `w` desliga a quebra e `h`/`l` rolam na horizontal (a busca, nesse modo, só enxerga o trecho visível).
- A busca opera nas linhas já renderizadas: acha texto que atravessa trechos estilizados (`foo**bar**`), mas não uma frase que a largura da tela quebrou em duas linhas.
- Imagens usam meio-bloco (baixa resolução); para mais nitidez use `--graphics kitty` (Sixel não é suportado).
- A memória livre do computador só é informada no Linux.
- Quando o arquivo muda, as linhas novas ou alteradas ganham uma barra amarela `▌` na margem esquerda, relativa à versão imediatamente anterior. Remoções não são marcadas. Acima de 20 000 linhas o `mdv` não calcula a diferença. No `--plain` não há marcas.
- `y`/`Y` dependem de o terminal aceitar OSC 52 (no tmux, `set -g set-clipboard on`); terminais sem suporte ignoram em silêncio.

## Changelog

<!-- mdv:changelog:start -->
### v0.3.0

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
