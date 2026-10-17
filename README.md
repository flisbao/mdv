# mdv

**Leia Markdown no terminal como deveria ser: com cor, estrutura e sem sair do teclado.**

`mdv` transforma qualquer arquivo `.md` numa página bem formatada direto no terminal — títulos em destaque, tabelas alinhadas, listas aninhadas, código colorido — e abre num leitor interativo com busca, navegação por títulos e recarga com `r`. Um único binário, sem runtime, sem navegador.

[![release](https://img.shields.io/github/v/release/flisbao/mdv)](https://github.com/flisbao/mdv/releases/latest)
[![ci](https://github.com/flisbao/mdv/actions/workflows/ci.yml/badge.svg)](https://github.com/flisbao/mdv/actions/workflows/ci.yml)

Além de Markdown, abre **JSON, CSV e TSV** como dados formatados, desenha **imagens e diagramas Mermaid** no próprio terminal e acompanha a saída de **agentes e LLMs** em tempo real (`--follow`).

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
- **Tabelas que cabem na tela.** Colunas se ajustam à largura disponível; células largas quebram em várias linhas (até 8, depois `…`). Na TUI, `t` abre a tabela na largura inteira do terminal, com cabeçalho e primeira coluna fixos, `h`/`l` para rolar as colunas e `n`/`p` para trocar de tabela; `--table N` abre direto numa delas.
- **Quebra de linha correta.** O texto é quebrado na largura do terminal respeitando caracteres largos: acentos, emojis e CJK (日本語) não desalinham nada.
- **Leitor interativo.** Rolagem, página, meia página, busca com `/`, `n`/`N` entre ocorrências e `]`/`[` para pular de título em título.
- **Imagens no terminal.** `![alt](arquivo.png)` e, com `--remote`, imagens por URL, desenhadas em meio-bloco, sem protocolo gráfico. [Detalhes](#imagens).
- **Diagramas Mermaid.** Blocos <code>```mermaid</code> viram imagem com o renderizador embutido, sem instalar nada, automaticamente quando o terminal tem protocolo gráfico (Kitty/Ghostty); sem ele, ou com `mermaid = off` na [configuração](#configuração), ficam como código. [Detalhes](#mermaid).
- **Andamento de tarefas.** Listas `- [x]`/`- [ ]` mostram o progresso: o rodapé da leitura traz `☑ feitas/total` do documento e da seção atual, e o `--toc` acrescenta `[feitas/total]` aos títulos. Sem tarefas no documento (e no modo fonte, JSON e CSV) nada disso aparece.
- **Feito para saída de agentes e LLMs.** Front matter YAML, alertas GFM, notas de rodapé e `<details>` aparecem formatados; `--follow` e `--toc` acompanham e resumem. Na TUI, `m` alterna para a fonte crua. [Detalhes](#markdown-de-agentes-e-llms).
- **JSON, CSV e TSV.** `mdv json|csv|tsv` com `--as table|schema|text`, `--path`, `--head`, `--cols`, `--no-header` e `--delimiter`. [Detalhes](#json-csv-e-tsv).
- **Recarga com `r`.** Edite o arquivo no seu editor e aperte `r` no `mdv` ao lado para ver o resultado. Ótimo para escrever README.
- **Funciona em pipe.** Com `-p` ou quando a saída não é um terminal, imprime texto com ANSI. Serve em scripts, `less -R` e CI.
- **Responsivo.** Redesenha ao redimensionar a janela; `-w` fixa a largura máxima.
- **Seguro com entrada não confiável.** Caracteres de controle e sequências de escape do documento são neutralizados, então um `.md` malicioso não consegue mexer no seu terminal. Tamanho de entrada é limitado, e a rede só entra com `--remote`. Veja as [pendências conhecidas](docs/pendencias.md).
- **Rápido e portátil.** Binário único em Rust para Linux (glibc e estático musl), macOS e Windows.

## Instalação

Baixe o binário da última release, extraia e coloque `mdv` no seu `PATH`:

<!-- mdv:install:start -->
| Plataforma | Download |
|------------|----------|
| Linux x86_64 (glibc 2.35+) | [`mdv-v0.4.4-linux-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.4.4/mdv-v0.4.4-linux-x86_64.tar.gz) |
| Linux x86_64 (estático, qualquer distro, Alpine inclusive) | [`mdv-v0.4.4-linux-x86_64-musl.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.4.4/mdv-v0.4.4-linux-x86_64-musl.tar.gz) |
| Linux aarch64 | [`mdv-v0.4.4-linux-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.4.4/mdv-v0.4.4-linux-aarch64.tar.gz) |
| macOS Apple Silicon | [`mdv-v0.4.4-macos-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.4.4/mdv-v0.4.4-macos-aarch64.tar.gz) |
| macOS Intel | [`mdv-v0.4.4-macos-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.4.4/mdv-v0.4.4-macos-x86_64.tar.gz) |
| Windows x86_64 | [`mdv-v0.4.4-windows-x86_64.zip`](https://github.com/flisbao/mdv/releases/download/v0.4.4/mdv-v0.4.4-windows-x86_64.zip) |
| Debian/Ubuntu e Fedora/RHEL | pacotes `.deb` e `.rpm` na [página da release](https://github.com/flisbao/mdv/releases/tag/v0.4.4) |

Versão atual: **v0.4.4**. Cada release traz um `SHA256SUMS` para conferir os arquivos.
<!-- mdv:install:end -->

Os scripts abaixo detectam o sistema e a arquitetura, baixam da **última release publicada** o arquivo certo e instalam o `mdv`. A tag vem do redirecionamento de `releases/latest` (Linux e macOS, sem depender da API do GitHub) ou da API (Windows). Os binários de macOS e Windows **não são assinados**: veja o aviso de cada sistema.

### Linux

Escolhe `x86_64` (binário estático musl, roda em qualquer distro) ou `aarch64` (glibc 2.35+) e instala em `/usr/local/bin`:

```bash
(
  set -eu
  case "$(uname -m)" in
    x86_64 | amd64)  alvo=linux-x86_64-musl ;;
    aarch64 | arm64) alvo=linux-aarch64 ;;
    *) echo "arquitetura não suportada: $(uname -m)" >&2; exit 1 ;;
  esac
  tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/flisbao/mdv/releases/latest)
  tag=${tag##*/}
  cd "$(mktemp -d)"
  curl -fsSL "https://github.com/flisbao/mdv/releases/download/${tag}/mdv-${tag}-${alvo}.tar.gz" | tar -xz
  sudo install -m 755 "$(find . -type f -name mdv | head -n1)" /usr/local/bin/mdv
  mdv --version
)
```

Em Debian/Ubuntu e Fedora/RHEL também há pacotes `.deb` e `.rpm` na [página da release](https://github.com/flisbao/mdv/releases/latest).

### macOS

Escolhe Apple Silicon (`arm64`) ou Intel (`x86_64`) e instala em `/usr/local/bin`:

```bash
(
  set -eu
  case "$(uname -m)" in
    arm64)  alvo=macos-aarch64 ;;
    x86_64) alvo=macos-x86_64 ;;
    *) echo "arquitetura não suportada: $(uname -m)" >&2; exit 1 ;;
  esac
  tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/flisbao/mdv/releases/latest)
  tag=${tag##*/}
  cd "$(mktemp -d)"
  curl -fsSL "https://github.com/flisbao/mdv/releases/download/${tag}/mdv-${tag}-${alvo}.tar.gz" | tar -xz
  bin=$(find . -type f -name mdv | head -n1)
  xattr -d com.apple.quarantine "$bin" 2>/dev/null || true
  sudo mkdir -p /usr/local/bin
  sudo install -m 755 "$bin" /usr/local/bin/mdv
  mdv --version
)
```

**Gatekeeper:** o binário não é assinado. Se você baixar pelo navegador e o sistema bloquear, remova a quarentena com `xattr -d com.apple.quarantine mdv`.

### Windows

No PowerShell (5.1 ou 7+), instala em `%LOCALAPPDATA%\mdv` e acrescenta a pasta ao `PATH` do usuário. Só existe build `x86_64`; em Windows ARM64 ele roda pela emulação do sistema:

```powershell
$ErrorActionPreference = 'Stop'
if ($env:PROCESSOR_ARCHITECTURE -notin 'AMD64', 'ARM64') {
  throw "arquitetura não suportada: $env:PROCESSOR_ARCHITECTURE"
}
$tag  = (Invoke-RestMethod https://api.github.com/repos/flisbao/mdv/releases/latest).tag_name
$zip  = Join-Path $env:TEMP "mdv-$tag.zip"
$tmp  = Join-Path $env:TEMP "mdv-$tag"
$dest = Join-Path $env:LOCALAPPDATA 'mdv'
Invoke-WebRequest "https://github.com/flisbao/mdv/releases/download/$tag/mdv-$tag-windows-x86_64.zip" -OutFile $zip
Expand-Archive $zip -DestinationPath $tmp -Force
New-Item -ItemType Directory -Force $dest | Out-Null
$exe = Get-ChildItem $tmp -Recurse -Filter mdv.exe | Select-Object -First 1
Copy-Item $exe.FullName (Join-Path $dest 'mdv.exe') -Force
Unblock-File (Join-Path $dest 'mdv.exe')
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($path -split ';') -notcontains $dest) {
  [Environment]::SetEnvironmentVariable('Path', "$path;$dest", 'User')
}
$env:Path += ";$dest"
Remove-Item $zip, $tmp -Recurse -Force
mdv --version
```

**SmartScreen:** o binário não é assinado. Se aparecer o aviso de aplicativo desconhecido, escolha "Mais informações" e depois "Executar assim mesmo". Use o Windows Terminal para ver cores e a TUI corretamente. Abra um novo terminal para o `PATH` valer fora da sessão atual.

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
    mdv <pasta>            (seletor: .md da pasta por data, filtro ao digitar)
    (o formato também é deduzido da extensão: .json, .jsonl, .csv, .tsv)

OPÇÕES:
    -p, --plain        imprime no terminal (sem TUI), como `cat` com cores
    -w, --width <N>    largura máxima do texto (padrão: 100)
        --remote       baixa imagens de http(s):// (nunca de rede privada)
        --graphics <G> imagens em alta resolução: auto (padrão; detecta Kitty/Ghostty), kitty ou off (só no modo interativo)
        --toc          imprime só o índice de títulos
        --stats        imprime palavras, blocos de código e tokens estimados, no total e por seção
        --follow       com stdin: acompanha o texto conforme chega
        --table <N>    abre direto no zoom da N-ésima tabela (só no modo interativo)
        --memory-buffer <T>  limite de memória (padrão: 512M; ex.: 256M, 2G; ou $MDV_MEMORY_BUFFER)
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
    Tab / Shift-Tab    seleciona o próximo / anterior link; Enter segue (âncora, .md relativo ou URL http(s)/mailto com confirmação `y`), Esc cancela
    Backspace / H      volta ao documento anterior (pilha de até 100)
    m                  alterna entre renderizado e fonte crua
    w                  código sem quebra: h/l (ou ←/→) rolam 8 colunas, 0/$ início/fim; w de novo volta a quebrar
    v                  (JSON) alterna tabela / esquema / texto
    t                  zoom da tabela mais visível na tela (h/l colunas, n/p troca de tabela, Esc volta)
    i                  zoom do diagrama Mermaid mais visível na tela: n/p troca de diagrama, +/- zoom, h/j/k/l movem, espaço/b página, g/G topo/fim, 0 ajusta, Esc volta
    y / Y              copia o bloco de código da tela / o Markdown da seção (OSC 52, até 64 KiB)
    r                  recarrega o arquivo
    q, Esc             sai
```
<!-- mdv:help:end -->

## Imagens

Imagens `![alt](caminho)` aparecem desenhadas no próprio terminal, em **meio-bloco** (`▀` com cor de frente e de fundo), então funcionam na TUI e no `--plain` sem protocolo gráfico. Caminhos relativos são resolvidos a partir da pasta do arquivo (com stdin, do diretório atual).

- **Locais:** PNG, JPEG, GIF e WebP, até 10 MiB e 8192×8192 px. Se não abrir, aparece `[imagem: alt — motivo]`. Imagens embutidas como `data:image/...;base64,...` (mesmos limites) também funcionam, sem rede; até 32 por documento.
- **Por URL:** só com `--remote`. Sem a flag, o `mdv` nunca acessa a rede. O download recusa endereços de rede privada, loopback e link-local, limita o tamanho a 10 MiB, segue no máximo 3 redirecionamentos e aceita até 32 imagens por documento. Na TUI o texto aparece na hora e a imagem entra quando o download termina.
- **Alta resolução (automática):** o padrão é `--graphics auto`, que detecta Kitty e Ghostty fora do `tmux`/`screen`; `--graphics kitty` força e `--graphics off` desliga. Nesses terminais o `mdv` desenha as imagens e os diagramas Mermaid pelo protocolo gráfico do Kitty, com *placeholders Unicode*, então rolam e somem com o texto. Só vale no modo interativo: `--plain` e saída redirecionada seguem em meio-bloco para imagens, e os diagramas Mermaid ficam como código (o Mermaid só é desenhado com protocolo gráfico). Sixel não é suportado.
- **Não suportados:** SVG e `data:` sem `;base64`. A resolução é baixa (meio-bloco); imagem dentro de célula de tabela vira `[imagem: alt ↓n]` na célula e é desenhada logo abaixo da tabela, com a legenda `↓n alt`.

## Mermaid

Blocos <code>```mermaid</code> viram imagem **sem instalar nada** automaticamente quando o terminal suporta o protocolo gráfico do Kitty (detecção `--graphics auto`, que é o padrão; não precisa de flag); sem suporte gráfico (`--plain`, saída redirecionada, `--graphics off`, terminais sem Kitty) o bloco fica como código. O `mdv` traz um renderizador embutido (flowchart, sequence, class, state, ER, pie, Gantt, timeline, journey, mindmap e outros). Ele precisa de ao menos uma fonte instalada no sistema para desenhar o texto.

```bash
mdv doc.md                      # diagramas como imagem sozinhos, em terminal com Kitty/Ghostty
mdv doc.md                     # sem gráficos: o diagrama aparece como código
```

**Zoom (`i`).** No texto o diagrama cabe em até 120×36 células, o que num diagrama grande (uma sequência com muitos participantes, por exemplo) não dá para ler. Na TUI, `i` abre o diagrama que ocupa mais linhas da tela (a barra mostra `N/M` e o título da seção) usando o terminal inteiro; `n`/`p` passam ao próximo ou ao anterior, dando a volta nos extremos: `+`/`-` aproximam e afastam, `h`/`j`/`k`/`l` movem a janela, espaço/`b` rolam uma página, `g`/`G` vão ao topo e ao fim, `0` volta a mostrar tudo e `Esc` fecha. Com `--graphics kitty` o recorte é reenviado em resolução nativa (nítido); o zoom só abre diagramas desenhados como figura, isto é, com gráficos ativos (em meio-bloco só no caso raro de o cache gráfico estar cheio).

Se o renderizador embutido não conseguir desenhar um diagrama (muito complexo, ou sintaxe que ele não conhece), o bloco aparece como código, com o motivo no cabeçalho. Limites: 64 KiB, 400 linhas e 100 conexões (setas `>` e `&`) por diagrama, imagem de até 1200×3000 px.

Onde o diagrama aparece:

| Terminal | Sistemas | Como aparece |
|---|---|---|
| kitty, Ghostty | Linux, macOS | alta resolução com `--graphics auto` (confirmado) |
| WezTerm, iTerm2, Warp, Konsole | varia | código; `--graphics kitty` por sua conta (suporte a *placeholders* Unicode não confirmado) |
| Windows Terminal, demais terminais com cores 24 bits | Windows, Linux, macOS | código (o diagrama exige protocolo gráfico) |

> O renderizador embutido roda dentro do próprio `mdv`, sem processos externos, com tetos de tamanho, conexões e memória.

## Markdown de agentes e LLMs

Construções comuns em saídas de LLMs aparecem formatadas, sem tags cruas: front matter YAML (como bloco `yaml`), alertas GFM (`> [!WARNING]`), notas de rodapé e `<details>`/`<summary>`. Para acompanhar uma saída em andamento:

```bash
agente | mdv --follow          # o texto cresce ao vivo e a tela acompanha o fim
mdv --toc relatorio.md         # só o índice de títulos
```

Na TUI, a tecla `m` alterna entre o texto renderizado e a **fonte crua**.

## Extrair trechos

Para scripts e prompts, o `mdv-tools` imprime só o que você pede, em texto puro (sem cor):

```bash
mdv-tools extract-code plano.md                      # todos os blocos de código
mdv-tools extract-code --lang bash -n 2 plano.md     # o 2º bloco bash
mdv-tools section "Instalação" plano.md              # o Markdown da seção
mdv-tools extract-code --section "Instalação" plano.md | sh
```

`--lang` não distingue maiúsculas; `-n` conta a partir de 1, depois do filtro. O título casa por igualdade (sem distinguir caixa) ou por trecho único; se for ambíguo, o `mdv-tools` lista os candidatos e sai com código 1, o mesmo código de quando nada é encontrado.

Para dimensionar um documento antes de colá-lo num prompt, `mdv --stats plano.md` imprime palavras, blocos de código e tokens estimados, no total e por seção (a seção pai soma as subseções; `[feitas/total]` aparece quando há tarefas). Os tokens são uma estimativa: caracteres do texto, prosa e código, divididos por 4. Só vale para Markdown e não combina com `--toc` nem `--follow`.

## Conferir links

`mdv-tools check-links doc.md` confere, **sem rede**, os links do Markdown e imprime um problema por linha, em texto puro. Sai com código `0` se estiver limpo e `1` se achar algo, então serve em CI e em scripts:

```text
plano.md:12: âncora "#instalacao" não existe
plano.md:30: arquivo "docs/x.md" não encontrado
plano.md:55: cerca de código aberta aqui e sem fechamento
3 problemas
```

O que confere: âncoras `#título` (mesma regra de slug do GitHub, com `-1`, `-2` para títulos repetidos), arquivos e imagens relativos (resolvidos a partir da pasta do documento; com stdin, do diretório atual), âncoras em outro `.md` (`outro.md#título`), links de referência sem definição (`[texto][ref]`) e cercas de código sem fechamento. Ignora `http(s)://`, `mailto:`, `data:`, caminhos absolutos, UNC (`\\servidor\pasta`) e com unidade (`C:`), que nunca são consultados no disco (nem depois de decodificar `%XX`), e tudo que está dentro de blocos de código. Limites: 200 problemas listados, 20 000 links e 50 outros `.md` lidos (até 5 MiB cada). Só vale para Markdown.

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

## Configuração

Arquivo opcional `$XDG_CONFIG_HOME/mdv/config` (ou `~/.config/mdv/config`), com linhas `chave = valor` e `#` para comentários:

```
# desenha os blocos mermaid como imagem quando o terminal suporta (padrão: on)
mermaid = off
```

Com `mermaid = off` os blocos <code>```mermaid</code> ficam sempre como código. Chaves ou valores desconhecidos são ignorados.

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
- `y`/`Y` dependem de o terminal aceitar OSC 52 (no tmux, `set -g set-clipboard on`); terminais sem suporte ignoram em silêncio.

## Changelog

<!-- mdv:changelog:start -->
### v0.4.4

_2026-10-16_

- fix(imagens): cache despeja o que a renderização anterior não usou (editar diagramas com live reload não esgota mais o limite de 16)

### v0.4.3

_2026-10-15_

- fix(zoom): i/t abrem o bloco mais visível e n/p trocam de bloco
- fix(width): largura padrão acompanha o terminal em vez de 100 fixo
- feat(mermaid): zoom de diagrama (tecla i) e teto inline maior

### v0.4.2

_2026-10-15_

- chore(deps): ratatui 0.30 e resvg 0.48
- Added cmd line to install and move to path on linux distros

### v0.4.1

_2026-10-15_

- fix(main): saneia o nome de pasta/arquivo mais novo na barra de status
- docs: registra as correções do review da Fase 8 no CLAUDE.md e no PLANO.md
- fix(setup): drena a saída da sonda de versão para o filho não morrer de SIGPIPE
- fix(chat): neutraliza tags think/details/summary soltas nas falas
- fix(render): cabeçalho @@ ou diff encerra o hunk anterior no --split
- fix(mermaid): gancho de pânico instalado uma vez e mudo só na thread do render
- fix(browse): saneia URL, âncora e nomes de arquivo antes de exibir
- fix(browse): seguir link .md no chat abre como Markdown e voltar restaura o modo
- fix(ai): Esc mata o subprocesso, leitura com prazo e stderr drenado
- fix(links): is_confined recusa .. para links não saírem da pasta do documento

### v0.4.0

_2026-10-15_

- feat(ai): tecla ? envia a seção ao MDV_AI_CMD e mostra a resposta (F8.7)
- feat(chat): mdv chat renderiza transcrição JSONL de agente (F8.1)
- feat(picker): mdv <pasta> abre seletor de .md com filtro fuzzy; mailto só com campos de texto (F8.6)
- feat(tui): Tab seleciona links; Enter segue âncora, .md local ou URL externa com confirmação; Backspace volta (F8.6)
- feat(diff): mdv diff, blocos diff coloridos e --split em duas colunas (F8.2)
- feat(follow): --idle avisa com sino quando a atividade para (F8.5)
- fix(links): nunca consultar caminho absoluto, UNC ou com unidade no --check-links
- feat(links): --check-links valida offline links relativos, âncoras e cercas sem fechar (F8.4)
- feat(mermaid): renderizador nativo vira o padrão; --mermaid acrescenta o mmdc como reserva
- fix(deps): atualiza rustls para 0.23.46 (RUSTSEC-2026-0346, 0347, 0348, 0349)
- fix(mermaid): teto de 100 conexões no renderizador nativo; probe_version lê stdout em thread
- docs: amostra de diagramas Mermaid
- docs: Mermaid nativo, --setup-mermaid e tabela de terminais (F8.8)
- feat(cli): --setup-mermaid diagnostica o ambiente dos diagramas e lista terminais suportados
- feat(mermaid): --mermaid tenta o renderizador nativo e cai no mmdc
- feat(mermaid): tetos de tamanho, erro claro sem fontes e proteção contra pânico no renderizador nativo
- feat(mermaid): renderizador nativo (mermaid-rs-renderer + resvg) devolve PNG em memória
- feat(stats): --stats mostra palavras, blocos de código e tokens estimados por seção (F8.3)
- refactor: refinamentos pós-code review em base64, cell_size e terminal control
- feat(tui): --graphics kitty|auto envia imagens em alta resolução ao terminal (F6.R4)
- feat(imagem): cache gráfico e placeholders Kitty no render (F6.R4)
- feat(graphics): protocolo Kitty com placeholders Unicode, funções puras (F6.R4)
- feat(render): imagem em célula de tabela vira figura numerada abaixo da tabela (F6.R3)
- feat(imagem): data:image/...;base64 vira imagem (F6.R3)
- feat(tui): w liga código sem quebra e h/l rolam na horizontal (F6.R2)
- fix(tui): realce da busca atravessa trechos estilizados (F6.R1)
- fix(tui): Z mantém o título da seção e --latest zera dobras e marcas ao trocar de arquivo
- docs: marcação de mudanças no reload (F7.1)
- feat(tui): marca na margem o que mudou no reload, com } { e c (F7.1)
- feat(diff): linhas novas entre duas versões do fonte, mapeadas para as linhas renderizadas (F7.1)
- feat(tui): z dobra a seção e Z dobra ou abre todas (F7.3)
- feat(fold): linhas visíveis com seções dobradas, chave estável por título (F7.3)

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
cargo build --profile dist                # binário de distribuição (LTO completo, mais lento)
cargo run --release -- sample.md        # TUI
cargo run --release -- -p -w 60 sample.md   # saída plain
cargo test --workspace
cargo test --workspace --no-default-features   # núcleo sem Mermaid nem rede
cargo run --release -p mdv-tools -- --help   # o binário mdv-tools
```

### Features do Cargo

Todas vêm ligadas por padrão; `cargo build --release --no-default-features` gera um binário bem menor (cerca de 134 crates contra 241).

| Feature | O que liga | Sem ela |
|---------|-----------|---------|
| `images` | decodificação de imagens (sempre compilada; existe para `mermaid` e `remote` dependerem dela) | — |
| `mermaid` | diagramas Mermaid nativos (`mermaid-rs-renderer` + `resvg`) | o bloco ` ```mermaid ` sai como código |
| `remote` | `--remote` (`ureq` + `url`) | `--remote` é aceita, mas a imagem remota mostra `recompile com --features remote` |

O pipeline é um só: entrada (Markdown, JSON, CSV/TSV) → linhas do `ratatui` → TUI ou ANSI. `src/render/` renderiza; `src/main.rs` escolhe o modo e `src/app/` cuida da TUI, da busca e da recarga (`r`); `src/cli.rs` interpreta os argumentos; `src/image.rs`, `src/net.rs` e `src/mermaid_native.rs` fazem o I/O de imagens e diagramas (nunca dentro de `render/`); `src/stream.rs` trata `--follow`; `src/data.rs` trata JSON e CSV/TSV.

Os blocos de **instalação** e **uso** acima são gerados pela CI a cada release (`scripts/update-readme.sh`), a partir dos arquivos publicados e da saída real de `mdv --help`; não edite o que está entre os marcadores `<!-- mdv:… -->`.

## Removido na 0.5.0

Estas funcionalidades saíram para manter o `mdv` focado em **ler documentos**. O código continua disponível no commit `c6c7a01` (tag `v0.4.4`): `git show v0.4.4:src/<arquivo>.rs`.

| Funcionalidade | Onde estava |
|---|---|
| Dobra de seções (`z` / `Z`) | `src/fold.rs` |
| `mmdc`, `--mermaid`, `--setup-mermaid`, `MDV_MMDC` | `src/mermaid.rs`, `src/setup.rs` |
| Tecla `?` e `MDV_AI_CMD` | `src/ai.rs` |
| `mdv chat` | `src/chat.rs` |
| `<think>` / `<thinking>` | `src/render.rs` |
| `--idle` | `src/main.rs` |
| `--latest` | `src/main.rs`, `src/stream.rs` |
| Reload automático por `mtime` (a tecla `r` continua) | `src/main.rs` |
| Marcas de mudança e teclas `{` / `}` / `c` | `src/diff.rs` |
| `mdv diff` e `--split` (blocos ` ```diff ` continuam coloridos; `.diff` abre como Markdown) | `src/render.rs` |

Movidos para o binário `mdv-tools`: `--extract-code`, `--section` e `--check-links` (`mdv-tools extract-code | section | check-links`).

## Licença

MIT
