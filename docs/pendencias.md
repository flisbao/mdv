# Pendências conhecidas

Itens encontrados na revisão final da Fase 6 (v0.2.0) e deixados para depois. Nenhum bloqueia o uso normal; os de segurança só se aplicam a documentos não confiáveis.

## Segurança e robustez

- [ ] **CSV com milhões de colunas em uma linha** (`src/data.rs`). `MAX_DATA_BYTES` (5 MiB) limita o texto, mas não o número de colunas nem de células: uma única linha com ~2,5 M de vírgulas gera uma linha com milhões de colunas. Pôr teto de colunas (e de células) em `render_table_data`, com mensagem em português e teste.
- [ ] **Corrida entre checar e abrir a imagem** (`src/image.rs`, `load_local`). Se o arquivo for trocado por uma FIFO entre a checagem do tipo e o `open`, a leitura trava. Fechar de vez abrindo com `O_NONBLOCK` (precisa da crate `libc`, pois o valor da constante muda entre plataformas).
- [ ] `mdv --latest` imprime no stderr o nome de arquivo não UTF-8 sem sanitizar (`Failure` sem `sanitize`).
- [ ] `follow_latest` relê até 16 MiB a cada 250 ms se o arquivo mais novo for ilegível; lembrar a falha (caminho + mtime) e mostrar o erro na barra de status.
- [ ] `src/mermaid.rs`: o stderr do `mmdc` só é lido depois do fim (mais de 64 KiB trava até o timeout) e `fs::read(output)` não tem teto (usar `MAX_IMAGE_BYTES`).
- [ ] O `HELP` de `--mermaid` não avisa que o Chromium pode buscar recursos de rede a partir do texto do diagrama (só o README avisa).

## Comportamento

- [ ] O cache de imagens nunca é renovado: uma imagem criada ou editada depois do `.md` não aparece no live reload. Incluir o `mtime` na chave ou limpar as falhas ao recarregar.
- [ ] `--toc` com JSON/CSV e `--follow` com arquivo ou `-p` são ignorados sem aviso.
- [ ] `<think>` sem fechamento escurece o resto do documento, e `</think>` faz `pop` do topo da pilha de estilos (não necessariamente o do `think`).
- [ ] `MAX_DATA_BYTES` rejeita CSV grande mesmo com `--head` pequeno; o `csv::Reader` faz streaming, dá para parar após `head` linhas e só contar o resto.
- [ ] `mode_from_extension` (`src/cli.rs`) usa `rsplit('.')` no caminho inteiro: um arquivo chamado `csv` sem extensão muda o modo. Usar `Path::extension`.
- [ ] O JSON indentado é recalculado a cada redimensionamento; guardar o resultado por largura.

## Testes

- [ ] `imagem_remota_em_rede_privada_e_bloqueada` (`tests/cli.rs`) deveria afirmar "bloqueado": hoje também passaria com um simples "conexão recusada".
