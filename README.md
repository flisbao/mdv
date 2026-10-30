# mdv

**Leia Markdown no terminal como deveria ser: com cor, estrutura e sem sair do teclado.** E converta para PDF, Word, LibreOffice e planilhas quando precisar.

`mdv` transforma qualquer `.md` numa página bem formatada direto no terminal e abre num leitor interativo com busca, navegação por títulos e recarga com `r`. Também abre **JSON, YAML, CSV, TSV e HTML** (tabelas, esquema ou texto, com o erro de sintaxe apontado na linha), desenha **imagens e diagramas Mermaid**, acompanha a saída de **agentes e LLMs** (`--follow`) e **exporta** para PDF, DOCX, ODT, XLSX e ODS. Dois binários pequenos e independentes (`mdv` e `mdv-tools`), sem runtime, sem navegador.

[![release](https://img.shields.io/github/v/release/flisbao/mdv)](https://github.com/flisbao/mdv/releases/latest)
[![ci](https://github.com/flisbao/mdv/actions/workflows/ci.yml/badge.svg)](https://github.com/flisbao/mdv/actions/workflows/ci.yml)

```bash
mdv README.md                          # leitor interativo
mdv -p README.md                       # só imprime, como um `cat` colorido
agente | mdv --follow                  # acompanha um texto que ainda está chegando
mdv csv vendas.csv --cols produto,total
mdv json dados.json --path .itens[0]
mdv config.yaml                        # YAML: cor por tipo e erro de sintaxe na linha certa
mdv relatorio.md -o relatorio.pdf      # exporta (também .docx e .odt)
mdv vendas.csv -o vendas.xlsx          # CSV/TSV para planilha (também .ods)
```

## Por que usar

- **Renderização completa:** títulos, listas (inclusive de tarefas), tabelas alinhadas que cabem na tela, citações, alertas GFM, notas de rodapé, `<details>` e código com realce de sintaxe. Acentos, emojis e CJK não desalinham.
- **Leitor interativo:** rolagem, busca com `/`, `]`/`[` entre títulos, zoom de tabela (`t`) e de diagrama (`i`), `m` para ver a fonte crua, `r` para recarregar. Redesenha ao redimensionar.
- **Imagens e Mermaid** no próprio terminal. [Imagens](#imagens) · [Mermaid](#mermaid)
- **JSON, YAML, CSV e TSV** como tabela, esquema ou texto, com erro de sintaxe apontado na linha. [Detalhes](#json-yaml-csv-e-tsv)
- **Exporta** Markdown para PDF/DOCX/ODT, e tabelas (do CSV, TSV ou do próprio Markdown) para XLSX/ODS. [Detalhes](#exportar)
- **Feito para agentes e LLMs:** `--follow`, `--toc`, `--stats` e as ferramentas do `mdv-tools`. [Detalhes](#markdown-de-agentes-e-llms)
- **Funciona em pipe:** com `-p`, ou quando a saída não é um terminal, imprime ANSI (serve em scripts, `less -R` e CI).
- **Seguro com entrada não confiável:** sequências de escape do documento são neutralizadas, o tamanho de entrada e a memória têm teto, e a rede só entra com `--remote`.
- **Portátil:** binários em Rust, sem dependências de runtime, para Linux (glibc e musl), macOS e Windows.

## Instalação

Baixe o pacote da última release, extraia e coloque `mdv` e `mdv-tools` no seu `PATH`:

<!-- mdv:install:start -->
| Plataforma | Download |
|------------|----------|
| Linux x86_64 (glibc 2.35+) | [`mdv-v0.8.0-linux-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.8.0/mdv-v0.8.0-linux-x86_64.tar.gz) |
| Linux x86_64 (estático, qualquer distro, Alpine inclusive) | [`mdv-v0.8.0-linux-x86_64-musl.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.8.0/mdv-v0.8.0-linux-x86_64-musl.tar.gz) |
| Linux aarch64 | [`mdv-v0.8.0-linux-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.8.0/mdv-v0.8.0-linux-aarch64.tar.gz) |
| macOS Apple Silicon | [`mdv-v0.8.0-macos-aarch64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.8.0/mdv-v0.8.0-macos-aarch64.tar.gz) |
| macOS Intel | [`mdv-v0.8.0-macos-x86_64.tar.gz`](https://github.com/flisbao/mdv/releases/download/v0.8.0/mdv-v0.8.0-macos-x86_64.tar.gz) |
| Windows x86_64 | [`mdv-v0.8.0-windows-x86_64.zip`](https://github.com/flisbao/mdv/releases/download/v0.8.0/mdv-v0.8.0-windows-x86_64.zip) |
| Debian/Ubuntu e Fedora/RHEL | pacotes `.deb` e `.rpm` na [página da release](https://github.com/flisbao/mdv/releases/tag/v0.8.0) |

Versão atual: **v0.8.0**. Cada release traz um `SHA256SUMS` para conferir os arquivos.
<!-- mdv:install:end -->

Instaladores de uma linha (o `mdv --version` roda no fim; os binários de macOS e Windows **não são assinados**):

```bash
# Linux e macOS: detecta sistema e arquitetura e instala em /usr/local/bin
(
  set -eu
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64 | Linux-amd64)   alvo=linux-x86_64-musl ;;
    Linux-aarch64 | Linux-arm64)  alvo=linux-aarch64 ;;
    Darwin-arm64)                 alvo=macos-aarch64 ;;
    Darwin-x86_64)                alvo=macos-x86_64 ;;
    *) echo "sistema/arquitetura não suportados: $(uname -sm)" >&2; exit 1 ;;
  esac
  tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/flisbao/mdv/releases/latest)
  tag=${tag##*/}
  cd "$(mktemp -d)"
  curl -fsSL "https://github.com/flisbao/mdv/releases/download/${tag}/mdv-${tag}-${alvo}.tar.gz" | tar -xz
  bin=$(find . -type f -name mdv | head -n1)
  xattr -d com.apple.quarantine "$bin" 2>/dev/null || true   # só macOS (Gatekeeper)
  sudo install -m 755 "$bin" /usr/local/bin/mdv
  mdv --version
)
```

```powershell
# Windows (PowerShell): instala em %LOCALAPPDATA%\mdv e acrescenta ao PATH do usuário
$ErrorActionPreference = 'Stop'
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
if (($path -split ';') -notcontains $dest) { [Environment]::SetEnvironmentVariable('Path', "$path;$dest", 'User') }
$env:Path += ";$dest"
Remove-Item $zip, $tmp -Recurse -Force
mdv --version
```

No macOS, se o Gatekeeper bloquear o binário baixado pelo navegador: `xattr -d com.apple.quarantine mdv`. No Windows, se o SmartScreen avisar, escolha "Mais informações" e "Executar assim mesmo"; use o Windows Terminal para cores e TUI corretas. Pacotes `.deb` e `.rpm` estão na [página da release](https://github.com/flisbao/mdv/releases/latest). Ou, com Rust estável:

```bash
cargo install --git https://github.com/flisbao/mdv
```

## Dois binários, cada um com seu trabalho

Cada release traz dois executáveis. Eles não dependem um do outro: instale só o que for usar.

| | `mdv` | `mdv-tools` |
|---|---|---|
| Para que serve | **ler** e **converter** documentos | **extrair** pedaços e **conferir** Markdown |
| Quem usa | você, no terminal | scripts, CI e agentes de IA |
| Saída | TUI colorida, ANSI ou arquivo (PDF, DOCX, ODT, XLSX, ODS) | texto puro, sem cor |
| Peso | maior (Mermaid, imagens, TLS) | mínimo (só o analisador de Markdown) |

### `mdv`: o leitor

Abre Markdown, JSON, YAML, CSV e TSV num leitor interativo (busca, títulos, links, zoom de tabela e de diagrama), imprime ANSI com `-p`, acompanha texto que ainda está chegando (`--follow`), mostra índice e estatísticas (`--toc`, `--stats`) e exporta com `-o`.

```bash
mdv README.md                      # leitor interativo
mdv -p README.md                   # imprime colorido, sem TUI
mdv relatorio.md -o relatorio.pdf  # exporta (também .docx, .odt, .xlsx, .ods)
```

### `mdv-tools`: a caixa de ferramentas

Imprime só o que você pede, sem cor, pronto para ir a um pipe. Lê de arquivo ou de stdin (`-`).

| Subcomando | O que faz |
|---|---|
| `extract-code` | imprime só o texto dos blocos de código (`--lang`, `-n`, `--section`) |
| `section <título>` | imprime o Markdown de uma seção |
| `check-links` | confere offline âncoras, arquivos e imagens relativos e cercas sem fechar; sai com `1` se achar problema |

```bash
mdv-tools extract-code --lang bash -n 2 plano.md        # o 2º bloco bash
mdv-tools section "Instalação" plano.md                 # só aquela seção
mdv-tools extract-code --section "Instalação" plano.md | sh
mdv-tools check-links doc.md                            # use no CI
```

### Por que dois binários?

O `mdv` começou como um leitor simples e foi ganhando de tudo um pouco: chat com IA, diff entre versões, dobra de seções, `mmdc` para diagramas, recarga automática. Cada ideia era boa sozinha, mas juntas viraram um canivete suíço que não cortava bem nada. O build deu o recado: uma compilação limpa levava quase 2 minutos, e boa parte vinha de Mermaid e TLS, que quem só queria extrair um bloco de código nunca usaria. Hoje, medido em release (12 núcleos), o `mdv` compila do zero em ~1min35 e gera 14 MB; o `mdv-tools` compila em ~6 s e tem menos de 1 MB.

Na 0.5.0 veio a faxina. O `mdv` voltou a fazer uma coisa só: **ler documentos** (e, de quebra, convertê-los). As ferramentas de script, que eram flags soltas (`--extract-code`, `--section`, `--check-links`), ganharam casa própria no `mdv-tools`. O texto que os dois precisam (saneamento, títulos, links) foi para a biblioteca `mdv-text`, e a exportação ficou na `mdv-export`. Ninguém copia código, e o `mdv-tools` nunca depende do `mdv`.

No fim, o que antes era uma ferramenta enorme virou duas pequenas: uma para você, outra para os seus scripts. Um CI que só confere links não baixa um leitor de 14 MB, e quem lê Markdown não carrega ferramentas que nunca vai chamar.

## Uso

<!-- mdv:help:start -->
```text
mdv - visualizador de Markdown no terminal

USO:
    mdv [opções] <arquivo.md>
    cat arquivo.md | mdv
    mdv json [opções] <arquivo|->
    mdv yaml [opções] <arquivo|->
    mdv csv|tsv [opções] <arquivo|->
    mdv [opções] <arquivo.md> -o <saída.pdf|docx|odt|xlsx|ods>
    mdv [opções] <email.msg> [-o <saída.md|pdf|docx|odt>]
    mdv csv|tsv [opções] <arquivo|-> -o <saída.xlsx|ods>
    mdv <pasta>            (seletor: .md, .msg, .yaml/.yml e .html/.htm da pasta por data, filtro ao digitar)
    (o formato também é deduzido da extensão: .json, .jsonl, .yaml, .yml, .csv, .tsv, .html, .htm)
    HTML (.html, .htm): páginas com cores do CSS, sem JavaScript; é uma aproximação, não um navegador

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
    -o, --output <A>   converte e sai (sem TUI); o formato vem da extensão de A:
                         .md  → .pdf .docx .odt (documento) ou .xlsx .ods (só as tabelas)
                         .csv/.tsv → .xlsx .ods
                         .msg → .md .pdf .docx .odt .xlsx .ods
        --force        com -o: sobrescreve o arquivo de destino se ele já existir
        --section <T>  com -o e Markdown → planilha: só as tabelas da seção de título T
                       (com --table N: só a N-ésima tabela do documento)
    -h, --help         mostra esta ajuda
    -V, --version      mostra a versão

OPÇÕES DE JSON E YAML (só valem para estes formatos):
        --path <p>     mostra só um trecho, ex.: .itens[0].nome
        --as <vista>   auto (padrão), text, table ou schema

OPÇÕES DE CSV/TSV (só valem para este formato):
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
    v                  (JSON/YAML) alterna tabela / esquema / texto
    t                  zoom da tabela mais visível na tela (h/l colunas, n/p troca de tabela, Esc volta)
    i                  zoom do diagrama Mermaid mais visível na tela: n/p troca de diagrama, +/- zoom, h/j/k/l movem, espaço/b página, g/G topo/fim, 0 ajusta, Esc volta
    y / Y              copia o bloco de código da tela / o Markdown da seção (OSC 52, até 64 KiB)
    r                  recarrega o arquivo
    q, Esc             sai
```
<!-- mdv:help:end -->

## Imagens

`![alt](caminho)` aparece desenhado no terminal em **meio-bloco** (funciona na TUI e no `--plain`). Caminhos relativos partem da pasta do arquivo (com stdin, do diretório atual).

- **Locais e `data:`:** PNG, JPEG, GIF e WebP, até 10 MiB e 8192×8192 px; até 32 por documento. Se não abrir, aparece `[imagem: alt — motivo]`. SVG não é suportado.
- **Por URL:** só com `--remote` (sem a flag o `mdv` nunca acessa a rede). Recusa rede privada, loopback e link-local, limita a 10 MiB, segue até 3 redirecionamentos.
- **Alta resolução:** `--graphics auto` (padrão) detecta Kitty e Ghostty e desenha pelo protocolo gráfico; `kitty` força, `off` desliga. Só no modo interativo. Sixel não é suportado.
- Imagem dentro de célula de tabela vira `[imagem: alt ↓n]` e é desenhada logo abaixo da tabela.

## Mermaid

Blocos <code>```mermaid</code> viram imagem com o renderizador embutido (flowchart, sequence, class, state, ER, pie, Gantt, timeline, journey, mindmap e outros), **sem instalar nada**. Na leitura, o diagrama só é desenhado em terminal com protocolo gráfico (Kitty/Ghostty); sem ele (`--plain`, saída redirecionada, `--graphics off`, `mermaid = off` na [configuração](#configuração)) o bloco fica como código. Na [exportação](#exportar) o diagrama é sempre desenhado. O renderizador precisa de ao menos uma fonte instalada no sistema.

Na TUI, `i` abre o diagrama mais visível em tela cheia: `n`/`p` trocam de diagrama, `+`/`-` aproximam e afastam, `h`/`j`/`k`/`l` movem, `0` mostra tudo, `Esc` fecha. Diagramas que o renderizador não consegue desenhar ficam como código, com o motivo. Limites: 64 KiB, 400 linhas e 100 conexões por diagrama; imagem de até 1200×3000 px.

## Markdown de agentes e LLMs

Front matter YAML, alertas GFM, notas de rodapé e `<details>`/`<summary>` aparecem formatados.

```bash
agente | mdv --follow          # o texto cresce ao vivo e a tela acompanha o fim
mdv --toc relatorio.md         # só o índice de títulos
mdv --stats plano.md           # palavras, blocos de código e tokens estimados, por seção
```

`--stats` estima tokens como caracteres ÷ 4; só vale para Markdown e não combina com `--toc` nem `--follow`.

**`mdv-tools`** imprime só o que você pede, em texto puro (sem cor):

```bash
mdv-tools extract-code --lang bash -n 2 plano.md     # o 2º bloco bash
mdv-tools section "Instalação" plano.md              # o Markdown da seção
mdv-tools extract-code --section "Instalação" plano.md | sh
mdv-tools check-links doc.md                         # confere links, sem rede
```

O título casa por igualdade (sem distinguir caixa) ou por trecho único; ambíguo ou ausente sai com código 1. `check-links` confere âncoras (regra de slug do GitHub), arquivos e imagens relativos, links de referência sem definição e cercas de código sem fechamento, imprime um problema por linha e sai com `1` se achar algo (serve em CI). Ignora `http(s)://`, `mailto:`, `data:`, caminhos absolutos, UNC e com unidade. Limites: 200 problemas, 20 000 links, 50 outros `.md` (até 5 MiB cada).

## E-mails `.msg` do Outlook

`mdv` abre um `.msg` direto: assunto, remetente, destinatários, anexos e corpo viram Markdown; tabelas do HTML viram tabelas e as respostas citadas viram citações. O seletor de pasta também lista `.msg`.

O repositório inclui `samples/email.msg`, uma mensagem fictícia que demonstra corpo formatado, tabela, resposta citada e anexo. Para experimentá-la:

```bash
mdv samples/email.msg                   # lê a amostra no terminal
mdv samples/email.msg -o email.md       # grava o Markdown (só para .msg; com --force sobrescreve)
mdv email.msg                           # abre outro e-mail
mdv email.msg -o email.pdf              # também .docx, .odt, .xlsx e .ods
```

A entrada é limitada a 32 MiB. Um `.msg` inválido sai com erro e código 1. O teste `sample_commitado_bate_com_o_gerador` do pacote `mdv-msg` verifica se a amostra commitada produz o mesmo Markdown que o gerador.

## JSON, YAML, CSV e TSV

```bash
mdv json itens.json                    # lista de objetos (e JSONL) vira tabela
mdv json dados.json --as schema        # só chaves e tipos (também: text, table)
mdv json dados.json --path .itens[0]   # só um trecho
mdv yaml deploy.yaml                   # YAML com cor por tipo (também: mdv deploy.yml)
mdv yaml deploy.yaml --as schema       # esquema do YAML
mdv yaml deploy.yaml --path .spec.replicas
mdv csv vendas.csv --head 20 --cols produto,total
mdv tsv planilha.tsv --no-header
mdv csv dados.txt --delimiter ';'
```

O formato também é deduzido da extensão (`.json`, `.jsonl`, `.yaml`, `.yml`, `.csv`, `.tsv`) e aceita `-` (stdin). JSON em lista de objetos vira tabela (objetos aninhados achatados como `endereco.cidade`); na TUI, `v` alterna tabela, esquema e texto (JSON e YAML). Tabelas mostram até 200 colunas e cortam células com mais de 500 caracteres (use `t` para o zoom ou `--cols`). A entrada é limitada a 5 MiB. `--path` e `--as` valem para JSON e YAML (com `--path`, o trecho do YAML aparece escrito como JSON); as opções de CSV/TSV (`--head`, `--cols`, `--no-header`, `--delimiter`) só para CSV/TSV.

**YAML.** A vista padrão (`auto`) é o texto original, com comentários e ordem preservados e cor por tipo (chaves, textos, números, `true`/`false`/`null`); só uma lista de objetos (como vários documentos `---`) abre como tabela, igual ao JSON, e `v` ou `--as text` mostram o texto. As vistas de tabela e de esquema leem o documento como dados: âncoras (`&`), aliases (`*`) e merge keys (`<<`) são resolvidos, tags como `!Ref` e `!Sub` (CloudFormation) são aceitas, vários documentos separados por `---` viram uma lista (como um manifesto Kubernetes com Deployment e Service) e chave repetida vale a última. O YAML 1.1 (`yes`/`no` como booleano) não é aplicado: só o esquema core do YAML 1.2 (a cor da vista de texto vem do realce de código e ainda pinta `yes`/`no` como booleano, mas a tabela e o esquema os tratam como texto). `mdv samples/` lista `.yaml` e `.yml` no seletor, junto com `.md`, `.msg`, `.html` e `.htm`. YAML ainda não exporta (`-o`).

**Erros de sintaxe (JSON e YAML).** Arquivo inválido não vira uma tela de erro vazia: o `mdv` mostra o texto original com a **linha do erro em fundo vermelho** e logo abaixo `✗ linha N, coluna M: mensagem` em português (por exemplo, `vírgula sobrando antes de fechar` no JSON ou `esperava uma chave nesta linha (verifique a indentação)` no YAML). Na TUI o documento abre já rolado até o erro, com algumas linhas de contexto acima; `m` mostra a fonte crua. Só erros de sintaxe são apontados, nunca estilo. Experimente `mdv samples/quebrado.yaml` e `mdv samples/quebrado.json`.

Para não estourar a máquina com entrada hostil, a leitura do YAML tem tetos: 2 milhões de nós vindos do próprio texto (folga para os 5 MiB de entrada), 1 milhão de nós criados por cópia (cada alias e cada âncora guardada, o que barra a "bomba de aliases"), 128 níveis de aninhamento e 32 MiB somados de chaves e textos. Passou de um teto, as vistas de tabela e de esquema avisam `estrutura grande demais: use a vista de texto`, e a vista de texto continua mostrando o original, com uma linha de aviso no topo (com `--path`, o aviso vira erro, pois o trecho não pode ser lido).

## HTML

`mdv pagina.html` lê uma página HTML no terminal: títulos, parágrafos, listas, tabelas, blocos de código, links e as **cores e estilos que o próprio arquivo declara** (`color`, `background`, negrito, itálico, sublinhado). Layout de bloco, `flex` e `grid` são aproximados.

```bash
mdv pagina.html                       # TUI, com as cores do CSS
mdv -p -w 100 relatorio.html          # imprime no terminal (cor só se a saída for um terminal)
mdv apresentacao.html                 # slides viram páginas; ] e [ pulam de slide
mdv docs/                             # seletor de pasta: lista também .html e .htm
```

- **Sem JavaScript, sem rede, sem CSS externo.** `<script>` não roda; imagens, folhas de estilo e fontes externas não são baixadas. Só valem o `<style>` e os `style="..."` do próprio arquivo.
- **Texto escondido não aparece** (`display:none`, `visibility:hidden`, `hidden`, `<template>`, `<noscript>`).
- **Links** funcionam como nos Markdown: `Tab` escolhe, `Enter` segue. Um `.md` ou `.html` relativo abre no próprio `mdv`, dentro da pasta do documento. `]`/`[` pulam entre títulos.
- **Apresentações** viram páginas navegáveis: cada slide é uma página, separada por uma régua, e `]`/`[` pulam de slide em slide. Uma página comum nunca é fatiada; para forçar a apresentação, marque o contêiner com `data-mdv-deck`.
- **É uma aproximação, não um navegador.** Pixels viram colunas e linhas, tamanhos de fonte viram só negrito e itálico, e efeitos visuais sem equivalente no terminal são simplificados ou deixados de fora. Use para ler o conteúdo, não para conferir a aparência.
- A tecla `m` mostra o HTML cru. A entrada por stdin é sempre Markdown, mesmo que o texto seja uma página. `-o` ainda não exporta HTML. Arquivos até 8 MiB.

## Exportar

`-o` converte e sai, sem TUI; o formato vem da extensão da saída.

```bash
mdv relatorio.md -o relatorio.pdf                    # também .docx e .odt
mdv email.msg -o email.md                            # .msg → Markdown (só .msg aceita -o .md)
mdv dados.csv -o dados.xlsx                          # também .ods; tsv igual
mdv relatorio.md -o tabelas.ods --section Vendas     # só as tabelas da seção "Vendas"
mdv relatorio.md -o tabela2.xlsx --table 2           # só a 2ª tabela do documento
```

| Entrada | Saída |
|---|---|
| Markdown | `.pdf`, `.docx`, `.odt` (o documento todo) |
| Markdown | `.xlsx`, `.ods` (só as tabelas, uma aba cada; `--section` e `--table` escolhem quais) |
| CSV / TSV | `.xlsx`, `.ods` |

- Um arquivo existente só é sobrescrito com `--force`; a gravação é atômica. Funciona com stdin (`cat doc.md | mdv -o doc.odt`).
- **Planilhas:** só texto e número (nada vira fórmula: `=1+1` fica como texto; `007` continua texto), cabeçalho em negrito e congelado. A aba leva o nome do título acima da tabela. Cortes nos limites do Excel (1 048 576 linhas, 16 384 colunas, 32 767 caracteres por célula) vêm com aviso. `--head` e `--cols` não valem com `-o`.
- **Imagens:** só caminhos relativos dentro da pasta do documento; caminho absoluto, `..` (também codificado) e links simbólicos que saem da pasta são recusados com aviso. Por URL, só com `--remote`. Diagramas Mermaid viram imagem (não precisa de terminal gráfico), exceto com `mermaid = off`, sem fontes no sistema ou binário sem a feature `mermaid`.
- **PDF:** fontes padrão; caracteres fora de Latin-1 viram `?`, com aviso; sem realce de sintaxe.

## Configuração

Arquivo opcional `$XDG_CONFIG_HOME/mdv/config` (ou `~/.config/mdv/config`), linhas `chave = valor` e `#` para comentários:

```
mermaid = off     # nunca desenha diagramas (padrão: on)
```

## Limite de memória

O `mdv` usa no máximo **512 MiB**. Se um arquivo precisar de mais, ele para e mostra a memória livre e o valor sugerido. Para arquivos maiores: `mdv --memory-buffer 2G grande.md` ou `MDV_MEMORY_BUFFER=2G` (a flag vence). Aceita `256M`, `1G`, `512MiB`; o mínimo é 32 MiB.

## Limitações conhecidas

- Código largo é quebrado na tela (`w` liga a rolagem horizontal; a busca, nesse modo, só enxerga o trecho visível).
- A busca opera nas linhas já renderizadas: não acha uma frase que a largura da tela quebrou em duas linhas.
- Imagens usam meio-bloco (baixa resolução); Sixel não é suportado.
- `y`/`Y` dependem de OSC 52 (no tmux, `set -g set-clipboard on`).

## Changelog

<!-- mdv:changelog:start -->
### v0.8.0

_2026-10-30_

- chore(release): prepara a versão 0.8.0
- test(html): 16 amostras e corpus com cinco critérios e goldens
- feat(html): abre .html, .htm e .xhtml na TUI e no --plain
- feat(html): crate mdv-html, HTML com CSS mínimo para células coloridas

### v0.7.1

_2026-10-29_

- chore(release): prepara a versão 0.7.1
- fix(deb): define o Maintainer do pacote .deb

### v0.7.0

_2026-10-29_

- chore(release): prepara a versão 0.7.0
- fix(yaml,erro): polimento dos itens menores adiados
- fix(erro): texto grande com erro de sintaxe só colore a linha do erro
- fix(yaml): teto estourado com --path é erro e a vista de texto avisa
- fix(yaml): teto de cópias separado do teto de nós do documento
- fix(erro): mensagem logo depois da parte da linha que tem a coluna do erro
- fix(testes): teste da bomba de aliases só roda no Linux
- docs(yaml): exemplos, goldens e documentação
- feat(seletor): lista arquivos YAML
- feat(yaml): variante Doc::Yaml na CLI, na TUI e no --plain
- feat(yaml): vistas de texto, tabela e esquema
- fix(yaml): conta no teto o alias para âncora ainda aberta
- feat(yaml): lê YAML para Value com limites e erro com posição
- feat(json): mostra o texto com a linha do erro de sintaxe
- feat(tui): rola até o erro de sintaxe ao abrir
- feat(erro): desenha erro de sintaxe sobre o texto original

### v0.6.0

_2026-10-21_

- docs: prepara release 0.6.0 da leitura msg
- docs(msg): traduz comentario de erro de leitura
- fix(msg): torna pre-varredura HTML conservadora
- fix(msg): isola orcamento da tentativa GFM
- fix(msg): propaga falha de memoria em embutidas
- fix(msg): conta blocos visiveis dentro de celulas
- fix(msg): acompanha saida de SVG e MathML
- fix(msg): propaga teto de bytes em anexos embutidos
- fix(msg): preserva texto escapado em celulas
- fix(msg): reconhece CDATA SVG no limite de profundidade
- fix(msg): pré-varredura de aninhamento conservadora (tokenizador, barreiras, formatação, tabelas)
- fix(msg): imagem data: só com rótulo, BOM fora, HTML UTF-8 sem charset, <br><br> vira parágrafo
- fix(msg): erro em anexo embutido só tira o corpo dele, sem abortar a leitura
- fix(msg): tabela com bloco de texto ou célula longa vira layout, sem cortar texto
- fix(msg): recusa HTML com aninhamento acima de 5000 antes do parser
- chore(msg): scraper sem features padrão (sem getopts nem errors)
- test(msg): amostra com imagem cid, <hr> fora da resposta e datas corretas
- fix(msg): <hr> no início do cabeçalho de resposta não vira citação
- test(msg): amostra samples/email.msg e golden
- fix(msg): acompanhamentos da revisão da Task 6
- feat(msg): mdv abre .msg e exporta Markdown com -o .md
- fix(msg): recusa de check_html cai no próximo passo do corpo
- refactor(msg): testes de markdown.rs em arquivo próprio
- fix(msg): quebras forçadas, orçamento que aborta e detalhes do cabeçalho
- feat(msg): montagem do Markdown do e-mail
- fix(msg): citação do Outlook, teto de saída e escape em tabelas
- feat(msg): conversão de HTML do Outlook em Markdown
- fix(msg): HTML em string com UTF-8, teto agregado de leitura e de embutidas
- feat(msg): leitura do contêiner .msg (MS-CFB/MS-OXMSG)
- feat(msg): HTML encapsulado em RTF (MS-OXRTFEX) e codepages
- fix(msg): limita COMPSIZE inferior ao cabeçalho e cobre volta do dicionário
- feat(msg): crate mdv-msg com descompressão RTF (MS-OXRTFCP)

### v0.5.2

_2026-10-20_

- fix(render): régua entre todas as linhas de dados da tabela

### v0.5.1

_2026-10-20_

- fix(render): régua entre as linhas de tabela com células quebradas

### v0.5.0

_2026-10-17_

- fix(export): Mermaid sem exigir gráficos, symlink para fora recusado, aviso de memória e fsync no -o
- fix(export): %00 recusado e testes de dupla codificação e hex inválido
- fix(export): nome da aba medido em unidades UTF-16 (teto do Excel)
- fix(export): espaço único após tab ou quebra de linha não colapsa no ODS
- test(export): script de validação com LibreOffice
- fix(export): confinar imagem também depois do percent-decode
- feat(cli): mdv -o converte Markdown em PDF/DOCX/ODT e tabelas em XLSX/ODS
- feat(cli): -o/--output, --force e --section viram Command::Export
- feat(export): convert despacha Markdown e CSV/TSV para os cinco formatos
- test(export): renomear teste de check_parse e documentar a ordem em from_markdown
- fix(export): table 0, ordem de check_parse e <br> em células de tabela
- feat(export): fontes de planilha: CSV/TSV e tabelas do Markdown por seção
- test(export): corrigir avisos do clippy nos testes do modelo de planilha
- feat(export): escritor ODS com mimetype primeiro e textos literais
- feat(export): escritor XLSX com cabeçalho congelado e textos literais
- feat(export): modelo de planilha com número estrito e nome de aba válido
- feat(export): portar escritores de PDF, DOCX e ODT para a crate mdv-export
- feat(export): esqueleto da crate mdv-export com Budget, PictureProvider e Format
- refactor(text): find_section, section_range e summary_text sobem para mdv-text
- fix: abrir link externo e achar config no Windows; mermaid_native::available substitui cfg! em image.rs
- fix(app): r avisa quando a releitura falha
- fix(stream): --follow avisa no texto quando a entrada passa do teto
- fix(stats): --stats respeita o limite de memória antes de analisar
- fix(image): decodifica percent-encoding em imagem local; data: gigante recusado antes do hash e sem copiar em acerto de cache
- fix(mdv-tools): cerca fechada em citação não é reportada como aberta; teste de nome com ESC só em unix
- fix(app): reenvia imagens Kitty ao trocar de arquivo; seletor só fecha se o arquivo abriu
- test(config): comparar caminhos com PathBuf para passar no Windows
- build: resvg 0.45 elimina duplicatas de fontdb/roxmltree; docs da fase 7
- build: ratatui sem features padrão
- ci: release e empacotamento usam o perfil dist
- build: release com LTO thin e perfil dist com LTO fat
- feat(mermaid): liga sozinho em terminal com Kitty; config mermaid = on|off
- ci: testa --no-default-features; documenta as features
- docs: images permanece sempre ligada (decisão da fase 6)
- feat(build): feature remote; --remote sem ela devolve erro claro
- feat(build): feature mermaid; sem ela o diagrama sai como código
- docs: ajuda e documentação do Cli por tipo (fase 5)
- refactor(cli): Cli por tipo (Common + Command), parser por formato
- test(cli): fixa as mensagens de erro antes da fase 5
- refactor(main): teto de tamanho dos arquivos e docs da fase 4
- refactor(app): teclas viram Action (input.rs)
- refactor(app): enum Modal substitui os cinco Option de modal
- refactor(app): App em src/app/ (viewport, search, zoom, ui, terminal)
- refactor(app): Document separa o conteúdo do estado de tela
- refactor(main): load.rs e plain.rs
- test(app): caracteriza o que cada modal captura (rede da fase 4)
- docs/test: alinha README e CLAUDE.md ao Mermaid só com gráficos; cobre erro com nativo real
- feat(render): Mermaid só é desenhado com gráficos ativos; docs da fase 3
- refactor(render): renderer.rs; mod.rs só com a API; teto de 500 linhas
- refactor(render): highlight.rs
- refactor(render): ImageProvider e images.rs; testes com fake
- refactor(render): inline.rs, list.rs e table.rs
- refactor(render): ansi.rs e wrap.rs
- refactor(render): model.rs, outline.rs e theme.rs
- refactor(render): render.rs vira render/mod.rs; testes em tests.rs e testutil.rs
- fix(tools): sanitiza nome de arquivo no rótulo e nos erros do mdv-tools
- docs: CI, empacotamento e documentação do workspace (fase 2)
- refactor: remove extract, links e as flags movidas para o mdv-tools
- feat(tools): binário mdv-tools com extract-code, section e check-links
- refactor: copia extract e links para a crate mdv-tools
- refactor: cria o workspace e a crate mdv-text
- docs: registra os cortes da fase 1 (README, CLAUDE.md)
- refactor: remove mmdc, --mermaid e --setup-mermaid
- refactor: remove a dobra de seções e a camada display
- refactor: remove o reload automático por mtime (r continua)
- refactor: remove marcas de mudança, { } c e a crate similar
- refactor: remove --latest
- refactor: remove --idle
- refactor: remove a tecla ? e MDV_AI_CMD
- refactor: remove o tratamento de <think>
- refactor: remove mdv chat
- refactor: remove mdv diff e --split
- test: goldens robustos a CRLF (.gitattributes) e vistas --as do JSON
- test: caracterização das teclas de rolagem, busca, saída e recarga
- test: goldens de ANSI com cor para o renderizador
- test: goldens de texto do modo --plain, --toc e --stats

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
cargo build --profile dist               # binário de distribuição (LTO completo)
cargo run --profile fast -- sample.md    # sem LTO: recompila rápido para testar à mão
cargo test --workspace
cargo test --workspace --no-default-features   # núcleo sem Mermaid nem rede
cargo run --release -p mdv-tools -- --help
```

Workspace com quatro crates, dois binários e duas bibliotecas: `mdv` (binário: CLI e TUI), `mdv-tools` (binário: extrair, seções e links), `mdv-text` (biblioteca: texto de Markdown compartilhado) e `mdv-export` (biblioteca: PDF/DOCX/ODT/XLSX/ODS). Features do Cargo, todas ligadas por padrão (`--no-default-features` gera um binário bem menor): `mermaid` (diagramas nativos) e `remote` (`--remote`); sem elas o bloco Mermaid sai como código e a imagem remota mostra `recompile com --features remote`. O pipeline é um só: entrada → linhas do `ratatui` → TUI ou ANSI; I/O de imagens, rede e diagramas nunca fica dentro de `src/render/`. Os blocos de instalação, ajuda e changelog são gerados pela CI a cada release (`scripts/update-readme.sh`): não edite o que está entre os marcadores `<!-- mdv:… -->`. `scripts/validar-exportacao.sh` confere as exportações com o LibreOffice (precisa de `libreoffice-calc` e `libreoffice-writer`).

## Removido na 0.5.0

Saíram para manter o `mdv` focado em **ler documentos** (a história completa está em [Dois binários](#dois-binários-cada-um-com-seu-trabalho)); o código está na tag `v0.4.4` (`git show v0.4.4:src/<arquivo>.rs`): dobra de seções (`z`/`Z`), `mmdc` e `--mermaid`/`--setup-mermaid`, tecla `?` e `MDV_AI_CMD`, `mdv chat`, `<think>`, `--idle`, `--latest`, reload automático por `mtime` (a tecla `r` continua), marcas de mudança (`{`/`}`/`c`) e `mdv diff`/`--split`. Movidos para o `mdv-tools`: `--extract-code`, `--section` e `--check-links`. (`--section` voltou ao `mdv`, mas só com `-o`, para exportar tabelas.)

## Licença

MIT
