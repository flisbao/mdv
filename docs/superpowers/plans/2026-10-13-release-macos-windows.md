# Release macOS e Windows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fazer o workflow de release existente (`.github/workflows/release.yml`) também compilar, empacotar e publicar binários de macOS (arm64 e x86_64) e Windows (x86_64), usando apenas runners gratuitos do GitHub Actions.

**Architecture:** Primeiro garantir que o código compila e passa nos testes nos três sistemas (novo `ci.yml` + teste de CLI portável). Depois estender a matriz do job `build` do `release.yml` com entradas macOS/Windows, mantendo o job `release` único (ele já baixa todos os artefatos, gera `SHA256SUMS` e cria a release). Um gatilho `workflow_dispatch` permite ensaiar o pipeline sem criar tag.

**Tech Stack:** GitHub Actions, Rust stable (`dtolnay/rust-toolchain`), `cargo`, runners `macos-latest` (arm64) e `windows-latest` (x86_64, MSVC), `gh` CLI.

**Spec:** Pedido do usuário ("pipeline que publique binários para macOS e Windows usando as máquinas free do GitHub") + `.github/workflows/release.yml` atual (Linux x86_64, aarch64, musl).

## Global Constraints

- Somente runners gratuitos hospedados pelo GitHub: `macos-latest`, `windows-latest`, `ubuntu-22.04` (nenhum runner `-large`/`-xlarge`, nenhum self-hosted).
- Sem segredos novos: nada de certificados de assinatura nem notarização (binários ficam sem assinatura; documentar isso).
- Nomes de artefatos seguem o padrão atual: `mdv-<tag>-<nome-da-matriz>.tar.gz`; no Windows o formato é `.zip`.
- O job `release` continua único e só roda em push de tag `v*`.
- Mensagens de commit, README e comentários do YAML em português, como no resto do projeto.
- Não adicionar dependências ao `Cargo.toml`.

## Review Focus

- Teste `arquivo_nao_regular_e_rejeitado` usa `/dev/zero`, que não existe no Windows: deve ser restrito a `cfg(unix)` (coberto na Task 1).
- `pipe_fechado_na_saida_nao_e_erro` pode se comportar diferente no Windows (erro de pipe fechado): o CI da Task 1 precisa rodá-lo e, se falhar, o código de saída deve ser tratado, não o teste silenciado.
- Binário empacotado no Windows precisa se chamar `mdv.exe` e o zip não pode conter caminhos absolutos ou barras invertidas quebradas: smoke test extrai o zip e executa o `.exe` (Task 2).
- Binário x86_64-apple-darwin é compilado de cruzamento num runner arm64: o smoke test deve executá-lo (via Rosetta) para provar que não é só um arquivo gerado (Task 2).
- Rodar o workflow numa branch (sem tag) não pode publicar release nem sobrescrever uma existente: job `release` condicionado a `refs/tags/v` (Task 2).

---

## File Structure

- Create: `.github/workflows/ci.yml` — build + testes em Linux, macOS e Windows a cada push/PR.
- Modify: `.github/workflows/release.yml` — matriz estendida, empacotamento por SO, smoke test, gatilho manual, release condicionada a tag.
- Modify: `tests/cli.rs:33-39` — teste de `/dev/zero` só em Unix.
- Modify: `README.md` — seção de instalação com binários de release e avisos de Gatekeeper/SmartScreen.

---

### Task 1: CI multiplataforma e testes portáveis

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `tests/cli.rs:33-39`

**Interfaces:**
- Consumes: nada.
- Produces: workflow `ci` com jobs `test (ubuntu-22.04|macos-latest|windows-latest)`; garante que `cargo test` passa nos três SOs antes de a Task 2 empacotar.

- [ ] **Step 1: Restringir o teste de `/dev/zero` a Unix**

Em `tests/cli.rs`, trocar:

```rust
#[test]
fn arquivo_nao_regular_e_rejeitado() {
```

por:

```rust
#[cfg(unix)]
#[test]
fn arquivo_nao_regular_e_rejeitado() {
```

- [ ] **Step 2: Conferir que continua passando localmente**

Run: `cargo test`
Expected: todos os testes passam (nenhum removido no Linux).

- [ ] **Step 3: Criar o workflow de CI**

Criar `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  test:
    name: test (${{ matrix.runner }})
    strategy:
      fail-fast: false
      matrix:
        runner: [ubuntu-22.04, macos-latest, windows-latest]
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2

      - name: Testes
        run: cargo test --release

      - name: Saída plain do sample
        shell: bash
        run: cargo run --release -- -p -w 60 sample.md
```

- [ ] **Step 4: Validar o YAML localmente**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo ok`
Expected: `ok`

- [ ] **Step 5: Commit e push da branch para rodar o CI**

```bash
git checkout -b ci/multiplataforma
git add .github/workflows/ci.yml tests/cli.rs
git commit -m "ci: rodar testes em Linux, macOS e Windows"
git push -u origin ci/multiplataforma
```

- [ ] **Step 6: Verificar o resultado nos três SOs**

Run: `gh run watch --exit-status $(gh run list --branch ci/multiplataforma --workflow ci --limit 1 --json databaseId -q '.[0].databaseId')`
Expected: os três jobs verdes. Se algum teste falhar no Windows/macOS (ex.: `pipe_fechado_na_saida_nao_e_erro`), corrigir o código em `src/main.rs` (tratamento de erro de escrita) em vez de ignorar o teste, e repetir os steps 5-6.

- [ ] **Step 7: Commit de eventuais correções**

```bash
git add -A
git commit -m "fix: ajustes de portabilidade encontrados pelo CI"
```
(Pular se o Step 6 passou de primeira.)

---

### Task 2: Matriz de release para macOS e Windows

**Files:**
- Modify: `.github/workflows/release.yml` (substituir o arquivo inteiro pelo conteúdo abaixo)

**Interfaces:**
- Consumes: CI verde da Task 1 (código compila e testa em macOS/Windows).
- Produces: artefatos `macos-aarch64`, `macos-x86_64`, `windows-x86_64` (além dos Linux); release com `mdv-<tag>-macos-*.tar.gz` e `mdv-<tag>-windows-x86_64.zip` mais `SHA256SUMS` cobrindo todos.

Decisões embutidas no YAML:
- macOS x86_64 é compilado por cruzamento no runner arm64 (`--target x86_64-apple-darwin`), evitando o runner Intel `macos-13`, aposentado. Roda via Rosetta, o que o smoke test aproveita.
- Windows só x86_64 (MSVC). `windows-11-arm` é gratuito apenas em repositórios públicos; fica fora (YAGNI) e pode entrar depois como mais uma linha da matriz.
- Entradas macOS/Windows usam `pkg: false` (sem deb/rpm).

- [ ] **Step 1: Reescrever `release.yml`**

```yaml
name: release

on:
  push:
    tags: ["v*"]
  # Ensaio manual: roda os builds e deixa os artefatos na execução, sem publicar release.
  workflow_dispatch:

permissions:
  contents: read

jobs:
  build:
    name: ${{ matrix.name }}
    strategy:
      fail-fast: false
      matrix:
        include:
          # glibc do runner define o piso de compatibilidade: 22.04 => glibc 2.35 (Ubuntu 22.04+, Debian 12+, Fedora 36+).
          - name: linux-x86_64
            runner: ubuntu-22.04
            musl: false
            pkg: true
          - name: linux-aarch64
            runner: ubuntu-22.04-arm
            musl: false
            pkg: true
          # Binário estático: roda em qualquer distro Linux x86_64, inclusive Alpine.
          - name: linux-x86_64-musl
            runner: ubuntu-22.04
            musl: true
            pkg: false
          # macos-latest é arm64 (Apple Silicon).
          - name: macos-aarch64
            runner: macos-latest
            musl: false
            pkg: false
          # Cruzamento a partir do runner arm64; o binário roda via Rosetta.
          - name: macos-x86_64
            runner: macos-latest
            musl: false
            pkg: false
            target: x86_64-apple-darwin
          - name: windows-x86_64
            runner: windows-latest
            musl: false
            pkg: false
            windows: true
    runs-on: ${{ matrix.runner }}
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.musl && 'x86_64-unknown-linux-musl' || matrix.target || '' }}

      - name: Instalar musl-tools
        if: matrix.musl
        run: sudo apt-get update && sudo apt-get install -y musl-tools

      - name: Build (musl, estático)
        if: matrix.musl
        run: |
          cargo build --release --target x86_64-unknown-linux-musl
          echo "BIN=target/x86_64-unknown-linux-musl/release/mdv" >> "$GITHUB_ENV"

      - name: Build (cruzado)
        if: ${{ matrix.target }}
        run: |
          cargo build --release --target "${{ matrix.target }}"
          echo "BIN=target/${{ matrix.target }}/release/mdv" >> "$GITHUB_ENV"

      - name: Build
        if: ${{ !matrix.musl && !matrix.target }}
        run: |
          cargo build --release
          if [ "${{ matrix.windows }}" = "true" ]; then
            echo "BIN=target/release/mdv.exe" >> "$GITHUB_ENV"
          else
            echo "BIN=target/release/mdv" >> "$GITHUB_ENV"
          fi

      - name: Instalar cargo-deb e cargo-generate-rpm
        if: matrix.pkg
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-deb,cargo-generate-rpm

      - name: Gerar .deb e .rpm
        if: matrix.pkg
        run: |
          cargo deb --no-build
          cargo generate-rpm

      - name: Empacotar (tar.gz)
        if: ${{ !matrix.windows }}
        run: |
          mkdir -p dist
          stage="mdv-${GITHUB_REF_NAME}-${{ matrix.name }}"
          mkdir "$stage"
          cp "$BIN" README.md "$stage/"
          tar -czf "dist/$stage.tar.gz" "$stage"
          if [ "${{ matrix.pkg }}" = "true" ]; then
            cp target/debian/*.deb target/generate-rpm/*.rpm dist/
          fi

      - name: Empacotar (zip)
        if: ${{ matrix.windows }}
        shell: pwsh
        run: |
          New-Item -ItemType Directory -Force dist | Out-Null
          $stage = "mdv-$env:GITHUB_REF_NAME-${{ matrix.name }}"
          New-Item -ItemType Directory $stage | Out-Null
          Copy-Item $env:BIN, README.md $stage
          Compress-Archive -Path $stage -DestinationPath "dist/$stage.zip"

      # Prova que o arquivo publicado extrai e executa, não só que o build terminou.
      - name: Testar binário empacotado
        run: |
          mkdir smoke
          if [ "${{ matrix.windows }}" = "true" ]; then
            7z x dist/*.zip -osmoke > /dev/null
            exe=$(ls smoke/mdv-*/mdv.exe)
          else
            tar -xzf dist/*.tar.gz -C smoke
            exe=$(ls smoke/mdv-*/mdv)
          fi
          printf '# Olá\n\n- item\n' | "$exe" -p - | grep -q "Olá"

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.name }}
          path: dist/*
          if-no-files-found: error

  release:
    name: publicar release
    needs: build
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-22.04
    permissions:
      contents: write
    env:
      GH_TOKEN: ${{ github.token }}
      GH_REPO: ${{ github.repository }}
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true

      - name: Gerar SHA256SUMS
        run: cd dist && sha256sum * > SHA256SUMS && cat SHA256SUMS

      - name: Criar release
        run: gh release create "$GITHUB_REF_NAME" dist/* --title "$GITHUB_REF_NAME" --generate-notes
```

- [ ] **Step 2: Validar o YAML localmente**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo ok`
Expected: `ok`

- [ ] **Step 3: Commit e push**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): publicar binários para macOS e Windows"
git push
```

- [ ] **Step 4: Ensaio manual sem tag**

Run: `gh workflow run release --ref ci/multiplataforma && sleep 5 && gh run watch --exit-status $(gh run list --workflow release --branch ci/multiplataforma --limit 1 --json databaseId -q '.[0].databaseId')`
Expected: os seis jobs `build` verdes; job `publicar release` aparece como *skipped*; `gh release list` não ganhou nenhuma release.

- [ ] **Step 5: Conferir os artefatos do ensaio**

Run: `gh run download $(gh run list --workflow release --branch ci/multiplataforma --limit 1 --json databaseId -q '.[0].databaseId') -D /tmp/ensaio && ls -R /tmp/ensaio`
Expected: `macos-aarch64/*.tar.gz`, `macos-x86_64/*.tar.gz`, `windows-x86_64/*.zip`, mais os Linux. (Em caso de falha, ver o log do step que falhou com `gh run view --log-failed`.)

---

### Task 3: Documentação e release de verdade

**Files:**
- Modify: `README.md` (nova seção "Instalação" antes de "Limitações conhecidas")

**Interfaces:**
- Consumes: workflow `release` funcionando (Task 2).
- Produces: documentação de instalação e primeira release com todos os SOs.

- [ ] **Step 1: Adicionar seção de instalação ao README**

Inserir antes de `## Limitações conhecidas`:

````markdown
## Instalação

Baixe o arquivo do seu sistema na [página de releases](../../releases) e confira o hash com `SHA256SUMS`.

| Sistema | Arquivo |
|---|---|
| Linux x86_64 / aarch64 | `mdv-<versão>-linux-*.tar.gz`, `.deb` ou `.rpm` |
| macOS Apple Silicon | `mdv-<versão>-macos-aarch64.tar.gz` |
| macOS Intel | `mdv-<versão>-macos-x86_64.tar.gz` |
| Windows x86_64 | `mdv-<versão>-windows-x86_64.zip` |

Os binários de macOS e Windows **não são assinados**:

- macOS: se o Gatekeeper bloquear o binário baixado pelo navegador, rode `xattr -d com.apple.quarantine mdv`.
- Windows: o SmartScreen pode avisar sobre um aplicativo desconhecido; escolha "Mais informações" > "Executar assim mesmo". Use um terminal moderno (Windows Terminal) para ver cores e a TUI corretamente.
````

- [ ] **Step 2: Commit, merge em `main` e abrir PR**

```bash
git add README.md
git commit -m "docs: instruções de instalação para macOS e Windows"
gh pr create --fill
```
Expected: CI do PR verde nos três SOs. Fazer o merge só com a aprovação do usuário.

- [ ] **Step 3: Alinhar a versão e criar a tag (pedir confirmação ao usuário antes)**

Publicar uma tag é uma ação externa e difícil de desfazer. Com `main` atualizado, e `version` do `Cargo.toml` igual à tag desejada:

```bash
git tag v0.1.0
git push origin v0.1.0
```

- [ ] **Step 4: Verificar a release publicada**

Run: `gh run watch --exit-status $(gh run list --workflow release --limit 1 --json databaseId -q '.[0].databaseId') && gh release view v0.1.0 --json assets -q '.assets[].name'`
Expected: lista inclui `macos-aarch64`, `macos-x86_64`, `windows-x86_64.zip` e `SHA256SUMS`.

---

## Self-Review

- Cobertura: macOS (arm64+x86_64) e Windows publicados pelo mesmo `release.yml` com runners gratuitos (Tasks 2); portabilidade validada antes (Task 1); docs (Task 3).
- Sem placeholders; nomes de matriz (`macos-aarch64`, `macos-x86_64`, `windows-x86_64`) consistentes entre YAML e README.
- Review Focus: `/dev/zero` (Task 1 Step 1), pipe fechado (Task 1 Step 6), `.exe` no zip e execução do binário cruzado (Task 2 smoke test), release só em tag (Task 2 `if` + Step 4).
