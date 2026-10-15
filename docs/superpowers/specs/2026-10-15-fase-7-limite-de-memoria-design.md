# F7.9 Limite de memória (`--memory-buffer`): design

## Objetivo e uso alvo

Um teste de estresse esgotou a memória do computador do desenvolvedor (um `flatten` copiava o prefixo de uma chave de 2 MB para cada um de 200 mil filhos, cerca de 400 GB). O mesmo risco existe para qualquer usuário que abra um arquivo hostil ou gigante: o `mdv` pode travar a máquina em vez de recusar a entrada.

Objetivo: o `mdv` limita o **próprio uso de memória** por padrão (512 MiB). Ao atingir o limite, para de processar, descarta o que estava montando e mostra no lugar do documento uma mensagem que diz o limite, quanta memória o computador tem livre e como aumentar o limite. Quem quiser ler arquivos maiores aumenta o limite com `--memory-buffer` ou `MDV_MEMORY_BUFFER`. Se o valor pedido for maior que a memória livre, avisa.

Sucesso: nenhuma entrada, por maior ou mais hostil que seja, leva o processo a usar muito mais que o limite; ao estourar, o usuário recebe uma mensagem acionável e o terminal continua utilizável.

## Decisões fixadas (com o usuário)

- Padrão de **512 MiB** (medido: um arquivo pequeno usa ~5 MiB; com bloco de código, ~50 MiB, porque o `syntect` carrega sintaxes e temas).
- Ao estourar: **parar e mostrar erro com a dica**. Nada de renderizar documento truncado.
- Aumentar o limite é decisão do usuário, mas o `mdv` avisa quando o valor excede a memória disponível.

## Abordagem escolhida: contar as alocações do processo

Alternativas descartadas: `setrlimit` (só Unix, exige dependência, conta memória virtual e mata o processo sem aviso) e estimar pelo tamanho do arquivo (a expansão do Markdown vai de ~10× a ~50× conforme o conteúdo).

Um alocador global (`#[global_allocator]`) em volta do `System` mantém o total de bytes vivos num `AtomicUsize`. Duas camadas usam esse número:

1. **Pontos de checagem** (`mem::check()`): um teste do contador contra o limite, chamado nos laços pesados. Ao estourar, o processamento devolve um erro e a tela mostra a mensagem. É o caminho normal.
2. **Teto duro no alocador**: acima de `limite × 3/2 + 16 MiB` a alocação é recusada (`null`) e o Rust aborta com a mensagem padrão. É só a rede de segurança para um ponto sem checagem que aloque de uma vez.

## Módulo `src/mem.rs`

Fica fora de `render.rs` (regra: I/O fora do renderizador). Leitura de `/proc/meminfo` e de variável de ambiente acontece só em `mem.rs` e `main.rs`.

- `CountingAlloc` (`GlobalAlloc`): `alloc`, `alloc_zeroed`, `dealloc` e `realloc` contam bytes; `realloc` delega ao `System.realloc` e ajusta o contador pela diferença, preservando o crescimento no lugar. Recusa quando `usado + pedido > HARD_CAP`.
- `DEFAULT_LIMIT = 512 MiB`, `MIN_LIMIT = 32 MiB`.
- `set_limit(bytes)`, `limit()`, `used()`; `hard_cap(limit) -> usize` (`const fn`, saturante).
- `check() -> Result<(), MemoryLimit>` e `exceeded() -> bool`; `check_against(used, limit)` pura, para teste.
- `check_input(len: u64) -> Result<(), MemoryLimit>`: recusa antes de ler um arquivo cujo tamanho × 4 já passa do limite.
- `parse_size(&str) -> Result<usize, String>`: inteiro seguido de `K|KB|KiB|M|MB|MiB|G|GB|GiB` (sem distinção de caixa, sempre múltiplos de 1024); sem sufixo = bytes. Erros de formato, zero, estouro e valor abaixo de `MIN_LIMIT`.
- `resolve(flag: Option<usize>, env: Option<&str>) -> Result<usize, String>`: flag vence variável de ambiente, que vence o padrão. Valor inválido na variável é erro que cita `MDV_MEMORY_BUFFER`.
- `available_bytes() -> Option<u64>`: `MemAvailable` de `/proc/meminfo` no Linux; `None` nas outras plataformas.
- `init()`: lê a memória disponível uma vez e guarda em `OnceLock`, para a mensagem de erro não fazer I/O.
- `MemoryLimit { used, limit }` com `Display`: usa `describe(limit, used, available)`.
- `warn_if_above_available(limit) -> Option<String>`.
- `fmt_size(bytes) -> String` (`512 MiB`, `20 GiB`, `3,1 GiB`) e `flag_size(bytes) -> String` (`2G`, `768M`).

### Mensagens

- Com memória conhecida e margem: `limite de memória de 512 MiB atingido ao processar este arquivo. O computador tem cerca de 20 GiB livres; para ler mais, use --memory-buffer 2G (ou MDV_MEMORY_BUFFER=2G).` A sugestão é `limite × 4`, no máximo 3/4 da memória livre.
- Sem margem (a sugestão não passa do limite atual): `... O computador tem só cerca de 400 MiB livres; não há margem segura para aumentar o limite.`
- Memória desconhecida: `... para ler mais, use --memory-buffer <TAMANHO> (por exemplo 2G).`
- Aviso de valor alto: `aviso: o limite de memória de 8G é maior que a memória livre do computador (3,1 GiB); o sistema pode ficar lento ou encerrar o programa.` Vai para o stderr em `--plain` e para o aviso da barra de status na TUI. Nunca bloqueia.

## Interface

- `--memory-buffer <TAMANHO>` (todos os modos) e `MDV_MEMORY_BUFFER`. Valor inválido é erro de uso (código 2): `--memory-buffer inválido: <v> (exemplos: 256M, 1G)`.
- `--help` e README documentam a flag, o padrão e a variável.

## Pontos de checagem

| Onde | Como |
|---|---|
| `render::render_with` | depois de cada evento do `pulldown-cmark`; ao estourar devolve `error_rendered(msg)` e descarta o `Renderer` |
| `render::layout_table` | por linha da tabela: `if mem::exceeded() { break }` (a checagem de evento seguinte devolve o erro) |
| `render::render_code` (realce por linha) | por linha, mesmo padrão |
| `data::render_json` | depois do parse, depois do `select` e antes de cada vista; `records_to_rows` interrompe por item |
| `data::render_table_data` | por registro do CSV |
| `main::read_markdown_file` | `mem::check_input(len)` antes de ler o arquivo (vale para `--latest` e reload) |
| `main::App::drain_stream` | ao estourar, para de acumular o stdin (`--follow`) e avisa na barra |

Os tetos que já existem (`MAX_INPUT_BYTES` 16 MiB, `MAX_DATA_BYTES` 5 MiB, `MAX_PRETTY_BYTES` 8 MiB, `MAX_TABLE_COLS`, `MAX_CELL_CHARS`, `MAX_KEY_NODES`, limites de imagem) continuam; o limite de memória é a rede de proteção global por cima deles.

## Fora do escopo

- Descobrir a memória livre no macOS e no Windows (exige dependência ou `unsafe`): nessas plataformas `available_bytes()` é `None` e as mensagens usam a forma sem número.
- Limite por tipo de recurso, decodificação incremental, streaming de arquivos grandes.
- Mudar os tetos de entrada existentes.

## Testes

- Unitários em `mem.rs`: `parse_size` (sufixos, caixa, zero, overflow, mínimo), `resolve` (precedência), `check_against`, `hard_cap` (inclusive saturação), `describe` e `fmt_size`/`flag_size` com e sem memória conhecida, `warn_if_above_available`, contador (`used()` >= bytes de um `Vec` vivo).
- O limite global **não** é alterado em testes unitários, que rodam em paralelo e compartilham o contador. O comportamento de estouro é testado em `tests/cli.rs`, executando o binário com `--memory-buffer 32M` e uma entrada de uns 4 MiB (expande para centenas de MiB, então estoura): sai a mensagem, o código é 0 e o processo não aborta.
- Precedência flag > variável de ambiente e erro de uso para valor inválido: `tests/cli.rs`.
- `App::stop_stream_for_memory` testado diretamente (função pura sobre o estado).

## Riscos e decisões

- O contador inclui o que o `syntect` mantém em `OnceLock` (~45 MiB depois do primeiro bloco de código). Com o padrão de 512 MiB isso é irrelevante; o mínimo de 32 MiB existe para o limite nunca ser menor que o consumo-base.
- O teto duro do alocador vale também para o binário de testes; o limite padrão (784 MiB de teto duro) cobre todos os testes atuais, que ficam abaixo de dezenas de MiB.
- `realloc` precisa preservar o realocador do sistema (crescimento no lugar); a implementação padrão de `GlobalAlloc::realloc` copiaria sempre.
- Código `unsafe` fica restrito ao `impl GlobalAlloc` (delegação direta ao `System`).
- Acesso ao contador usa `Ordering::Relaxed`: é uma estimativa de proteção, não sincronização.
