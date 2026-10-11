# Fase 6 — Novas features do `mdv` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fazer o `mdv` exibir diagramas Mermaid, imagens (locais e por URL), Markdown gerado por agentes/LLMs sem perdas, e objetos JSON/CSV/TSV por subcomandos próprios.

**Architecture:** O pipeline continua sendo "entrada → `Vec<Line>` do ratatui → TUI | ANSI". Imagens viram linhas de meio-bloco (`▀`, cor de frente = pixel de cima, fundo = pixel de baixo), então funcionam na TUI e no `--plain` sem protocolo gráfico. Tudo que faz I/O (disco, rede, subprocesso `mmdc`) fica fora de `render.rs`, num `ImageStore` com cache e threads de fundo; `render` só consulta o cache. JSON/CSV/TSV são módulos novos que reaproveitam `render_code` e `render_table`.

**Tech Stack:** Rust 2021, `ratatui 0.29`, `pulldown-cmark 0.13`, novos: `image 0.25`, `ureq 2` + `url 2`, `serde_json 1`, `csv 1`, `tempfile 3`. Mermaid via binário externo opcional `mmdc` (mermaid-cli).

**Spec:** `PLANO.md`, seção "Fase 6 — Novas features" (pedido original: Mermaid, imagens por URL, imagens, MD de agentes/LLMs, JSON/CSV/TSV com comandos próprios).

## Global Constraints

- Mensagens de erro, ajuda e README em português, com acentuação correta.
- **Pré-requisitos (Fases 1 e 5 do `PLANO.md`):** sanitização de caracteres de controle (item 1), clamp de `-w` (item 2), tabela sem CPU quadrática (item 3), leitura com teto de tamanho (item 4) e infraestrutura de testes (`cargo test` rodando). Esta fase assume `render::sanitize` aplicado num ponto único de entrada de texto.
- Rede e subprocesso são **opt-in** (`--remote`, `--mermaid`); sem as flags, nenhuma conexão e nenhum processo é criado.
- Limites fixos: imagem ≤ 10 MiB e ≤ 8192×8192 px; no máximo 32 imagens remotas por documento; entrada de dados (JSON/CSV) ≤ 5 MiB; fonte Mermaid ≤ 64 KiB.
- Testes não usam rede externa; só loopback (`127.0.0.1`).
- `cargo clippy` sem warnings novos; sem `unsafe`.
- Saída de `--plain` nunca contém `\x1b` além do SGR gerado pelo `mdv`.
- Interpretação assumida do item 4 do pedido ("visualizar todo o .MD elaborado por qualquer agente/LLM"): (a) renderizar sem perda as construções típicas de saída de LLM (front matter, alertas GFM, notas de rodapé, `<details>`, `<think>`), (b) alternar para a fonte crua (`m`), (c) `--toc`, (d) `--follow` para stdin em streaming, (e) `--latest <dir>` para acompanhar o `.md` mais novo de uma pasta. Se o objetivo era outro, ajustar só as Tasks 5–7.

## File Structure

| Arquivo | Responsabilidade |
|---|---|
| `src/cli.rs` (novo) | Parse de argumentos, subcomandos `json`/`csv`/`tsv`, inferência por extensão |
| `src/image.rs` (novo) | Decodificação com limites, renderização meio-bloco, `ImageStore` (cache + threads) |
| `src/net.rs` (novo) | Download HTTP(S) com política (sem rede privada, teto de bytes, redirecionamento manual) |
| `src/mermaid.rs` (novo) | Chamada ao `mmdc` com timeout, PNG de saída |
| `src/data.rs` (novo) | JSON/JSONL/CSV/TSV → `Rendered` |
| `src/stream.rs` (novo) | `Utf8Accum` e leitor de stdin em thread (`--follow`), `newest_md` (`--latest`) |
| `src/render.rs` | `RenderOpts`/`render_with`, imagens e Mermaid em `render_code`, compat de LLM, `render_code_only`, `render_table_only`, `toc` |
| `src/main.rs` | Usa `cli`, `App` ganha `ImageStore`, modo de conteúdo, tecla `m`, follow/latest |

Ordem de dependência: Task 1 → 2 → 3 → 4 (imagens/Mermaid); Tasks 5–7 (Markdown) só dependem da 1; Tasks 8–9 (dados) dependem da 1 e do passo 3 da Task 8.

---

### Task 1: CLI com subcomandos e flags novas

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs:1-105` (usar `cli::parse`; remover o `while let` manual), `HELP`

**Interfaces:**
- Produces:
  ```rust
  pub enum Mode { Markdown, Json, Table { delimiter: u8 } }
  pub struct Cli {
      pub plain: bool, pub width: Option<usize>, pub remote: bool, pub mermaid: bool,
      pub follow: bool, pub toc: bool, pub latest: bool,
      pub mode: Mode, pub path: Option<String>,
      pub json_path: Option<String>, pub head: usize,
      pub cols: Vec<String>, pub no_header: bool,
  }
  pub enum Parsed { Run(Cli), Help, Version }
  pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String>
  ```

- [ ] **Step 1: Escrever os testes que falham**

Criar `src/cli.rs` só com os testes (o módulo ainda sem implementação) e `mod cli;` em `main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Cli, String> {
        match parse(args.iter().map(|s| s.to_string()))? {
            Parsed::Run(c) => Ok(c),
            _ => Err("esperava Run".into()),
        }
    }

    #[test]
    fn markdown_por_padrao() {
        let c = run(&["a.md"]).unwrap();
        assert_eq!(c.mode, Mode::Markdown);
        assert_eq!(c.path.as_deref(), Some("a.md"));
    }

    #[test]
    fn subcomando_json_com_caminho() {
        let c = run(&["json", "d.json", "--path", ".a.b[0]"]).unwrap();
        assert_eq!(c.mode, Mode::Json);
        assert_eq!(c.json_path.as_deref(), Some(".a.b[0]"));
    }

    #[test]
    fn extensao_define_modo() {
        assert_eq!(run(&["x.csv"]).unwrap().mode, Mode::Table { delimiter: b',' });
        assert_eq!(run(&["x.tsv"]).unwrap().mode, Mode::Table { delimiter: b'\t' });
        assert_eq!(run(&["x.jsonl"]).unwrap().mode, Mode::Json);
    }

    #[test]
    fn tsv_subcomando_le_stdin() {
        let c = run(&["tsv", "-"]).unwrap();
        assert_eq!(c.mode, Mode::Table { delimiter: b'\t' });
        assert_eq!(c.path.as_deref(), Some("-"));
    }

    #[test]
    fn largura_invalida_e_erro() {
        assert!(run(&["-w", "abc", "a.md"]).is_err());
        assert!(run(&["-w"]).is_err());
    }

    #[test]
    fn largura_e_limitada() {
        assert_eq!(run(&["-w", "99999", "a.md"]).unwrap().width, Some(1000));
        assert_eq!(run(&["-w", "1", "a.md"]).unwrap().width, Some(20));
    }

    #[test]
    fn flag_desconhecida_e_erro() {
        assert!(run(&["--nada", "a.md"]).unwrap_err().contains("--nada"));
    }

    #[test]
    fn duplo_traco_aceita_caminho_com_hifen() {
        assert_eq!(run(&["--", "-x.md"]).unwrap().path.as_deref(), Some("-x.md"));
    }

    #[test]
    fn colunas_separadas_por_virgula() {
        let c = run(&["csv", "a.csv", "--cols", "id,nome"]).unwrap();
        assert_eq!(c.cols, vec!["id", "nome"]);
    }

    #[test]
    fn ajuda_e_versao() {
        assert!(matches!(parse(["-h".to_string()]), Ok(Parsed::Help)));
        assert!(matches!(parse(["-V".to_string()]), Ok(Parsed::Version)));
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test cli::`
Expected: erro de compilação (`parse`, `Cli`, `Mode` não definidos).

- [ ] **Step 3: Implementar**

Acima do módulo de testes, em `src/cli.rs`:

```rust
//! Parse manual de argumentos (sem dependência extra).

pub const MIN_WIDTH: usize = 20;
pub const MAX_WIDTH: usize = 1000;
pub const DEFAULT_HEAD: usize = 1000;
pub const MAX_HEAD: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Markdown,
    Json,
    Table { delimiter: u8 },
}

#[derive(Debug)]
pub struct Cli {
    pub plain: bool,
    pub width: Option<usize>,
    pub remote: bool,
    pub mermaid: bool,
    pub follow: bool,
    pub toc: bool,
    pub latest: bool,
    pub mode: Mode,
    pub path: Option<String>,
    pub json_path: Option<String>,
    pub head: usize,
    pub cols: Vec<String>,
    pub no_header: bool,
}

pub enum Parsed {
    Run(Cli),
    Help,
    Version,
}

fn mode_from_extension(path: &str) -> Mode {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "json" | "jsonl" | "ndjson" => Mode::Json,
        "csv" => Mode::Table { delimiter: b',' },
        "tsv" => Mode::Table { delimiter: b'\t' },
        _ => Mode::Markdown,
    }
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("a opção {flag} precisa de um valor"))
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut args = args.into_iter().peekable();
    let mut cli = Cli {
        plain: false, width: None, remote: false, mermaid: false, follow: false,
        toc: false, latest: false, mode: Mode::Markdown, path: None, json_path: None,
        head: DEFAULT_HEAD, cols: Vec::new(), no_header: false,
    };
    let mut explicit_mode = false;
    match args.peek().map(String::as_str) {
        Some("json") => { cli.mode = Mode::Json; explicit_mode = true; args.next(); }
        Some("csv") => { cli.mode = Mode::Table { delimiter: b',' }; explicit_mode = true; args.next(); }
        Some("tsv") => { cli.mode = Mode::Table { delimiter: b'\t' }; explicit_mode = true; args.next(); }
        _ => {}
    }

    let mut only_paths = false;
    while let Some(a) = args.next() {
        if only_paths || !a.starts_with('-') || a == "-" {
            if cli.path.replace(a).is_some() {
                return Err("só um arquivo por vez".into());
            }
            continue;
        }
        match a.as_str() {
            "--" => only_paths = true,
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-p" | "--plain" => cli.plain = true,
            "--remote" => cli.remote = true,
            "--mermaid" => cli.mermaid = true,
            "--follow" => cli.follow = true,
            "--toc" => cli.toc = true,
            "--latest" => cli.latest = true,
            "--no-header" => cli.no_header = true,
            "-w" | "--width" => {
                let v = value(&mut args, &a)?;
                let n: usize = v.parse().map_err(|_| format!("largura inválida: {v}"))?;
                cli.width = Some(n.clamp(MIN_WIDTH, MAX_WIDTH));
            }
            "--path" => cli.json_path = Some(value(&mut args, &a)?),
            "--head" => {
                let v = value(&mut args, &a)?;
                let n: usize = v.parse().map_err(|_| format!("--head inválido: {v}"))?;
                cli.head = n.clamp(1, MAX_HEAD);
            }
            "--cols" => {
                cli.cols = value(&mut args, &a)?
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
            }
            "--delimiter" => {
                let v = value(&mut args, &a)?;
                let b = match v.as_str() {
                    "\\t" | "tab" => b'\t',
                    s if s.len() == 1 && s.is_ascii() => s.as_bytes()[0],
                    _ => return Err(format!("delimitador inválido: {v}")),
                };
                cli.mode = Mode::Table { delimiter: b };
                explicit_mode = true;
            }
            other => return Err(format!("opção desconhecida: {other}")),
        }
    }
    if !explicit_mode {
        if let Some(p) = cli.path.as_deref() {
            cli.mode = mode_from_extension(p);
        }
    }
    Ok(Parsed::Run(cli))
}
```

Em `main.rs`, substituir o `while let` por:

```rust
let cli = match cli::parse(std::env::args().skip(1)) {
    Ok(cli::Parsed::Run(c)) => c,
    Ok(cli::Parsed::Help) => { print!("{HELP}"); return Ok(()); }
    Ok(cli::Parsed::Version) => { println!("mdv {}", env!("CARGO_PKG_VERSION")); return Ok(()); }
    Err(e) => { eprintln!("mdv: {e}\nuse --help para ver as opções"); std::process::exit(2); }
};
let (plain, width, path) = (cli.plain, cli.width, cli.path.clone());
```

(o restante de `main` segue usando `plain`, `width`, `path` até as tasks seguintes consumirem `cli`).

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test cli::`
Expected: 10 testes PASS.

- [ ] **Step 5: Verificação manual**

Run: `cargo run --release -- -p sample.md | head -5 && cargo run --release -- --nada; echo $?`
Expected: 5 primeiras linhas renderizadas; depois `mdv: opção desconhecida: --nada` e código `2`.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat(cli): subcomandos json/csv/tsv e flags novas com validação"
```

---

### Task 2: Imagens locais (renderização meio-bloco)

**Files:**
- Create: `src/image.rs`
- Modify: `Cargo.toml`, `src/render.rs` (`RenderOpts`, `render_with`, `Renderer<'a>`, `Tag::Image`/`TagEnd::Image`, `push_image`), `src/main.rs` (`mod image;`, `App.images`)
- Test: `#[cfg(test)]` em `src/image.rs` e `src/render.rs`

**Interfaces:**
- Produces:
  ```rust
  // image.rs
  pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
  pub const MAX_IMAGE_COLS: usize = 80;
  pub const MAX_IMAGE_ROWS: usize = 24;
  #[derive(Clone)] pub enum ImageState { Ready(Arc<DynamicImage>), Loading, Failed(String) }
  pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String>
  pub fn halfblock(img: &DynamicImage, cols: usize, max_rows: usize) -> Vec<Line<'static>>
  pub struct ImageStore { .. }
  impl ImageStore { pub fn new(base_dir: Option<PathBuf>) -> Self; pub fn get(&self, src: &str) -> ImageState }
  // render.rs
  pub struct RenderOpts<'a> { pub width: usize, pub images: Option<&'a ImageStore> }
  pub fn render_with(md: &str, opts: &RenderOpts) -> Rendered
  ```

- [ ] **Step 1: Dependências**

`Cargo.toml`:

```toml
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "gif", "webp"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Escrever testes que falham em `src/image.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn dois_pixels() -> DynamicImage {
        let mut b = image::RgbaImage::new(1, 2);
        b.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        b.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        DynamicImage::ImageRgba8(b)
    }

    pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba([9, 9, 9, 255])));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn meio_bloco_usa_pixel_de_cima_e_de_baixo() {
        let l = halfblock(&dois_pixels(), 10, 10);
        assert_eq!(l.len(), 1);
        let s = &l[0].spans[0];
        assert_eq!(s.content, "▀");
        assert_eq!(s.style.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(s.style.bg, Some(Color::Rgb(0, 0, 255)));
    }

    #[test]
    fn meio_bloco_reduz_para_caber() {
        let img = decode(&png_bytes(100, 100)).unwrap();
        let l = halfblock(&img, 10, 4);
        assert!(l.len() <= 4);
        assert!(l.iter().all(|x| x.spans.len() <= 10));
    }

    #[test]
    fn decode_rejeita_lixo() {
        assert!(decode(b"isto nao e imagem").is_err());
    }

    #[test]
    fn store_le_arquivo_relativo_a_base() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), png_bytes(4, 4)).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        assert!(matches!(store.get("a.png"), ImageState::Ready(_)));
    }

    #[test]
    fn store_falha_para_arquivo_inexistente_e_diretorio() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        assert!(matches!(store.get("nao-existe.png"), ImageState::Failed(_)));
        assert!(matches!(store.get("."), ImageState::Failed(_)));
    }

    #[test]
    fn store_nao_baixa_sem_remote() {
        let store = ImageStore::new(None);
        match store.get("https://example.com/a.png") {
            ImageState::Failed(m) => assert!(m.contains("--remote")),
            _ => panic!("deveria falhar"),
        }
    }
}
```

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo test image::`
Expected: erro de compilação.

- [ ] **Step 4: Implementar `src/image.rs`**

```rust
//! Decodificação de imagens e conversão para linhas de meio-bloco.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageReader, Limits};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_IMAGE_COLS: usize = 80;
pub const MAX_IMAGE_ROWS: usize = 24;
const MAX_DIMENSION: u32 = 8192;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;

#[derive(Clone)]
pub enum ImageState {
    Ready(Arc<DynamicImage>),
    Loading,
    Failed(String),
}

pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

/// Cada linha de texto mostra duas linhas de pixels: `▀` com cor de frente = pixel de cima
/// e fundo = pixel de baixo. Pixels com alfa < 128 ficam transparentes.
pub fn halfblock(img: &DynamicImage, cols: usize, max_rows: usize) -> Vec<Line<'static>> {
    let cols = cols.max(1) as u32;
    let max_h = (max_rows.max(1) * 2) as u32;
    let img = if img.width() > cols || img.height() > max_h {
        img.resize(cols, max_h, FilterType::Triangle)
    } else {
        img.clone()
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let px = |x: u32, y: u32| -> Option<Color> {
        let p = rgba.get_pixel(x, y).0;
        (p[3] >= 128).then_some(Color::Rgb(p[0], p[1], p[2]))
    };
    (0..h)
        .step_by(2)
        .map(|y| {
            let spans: Vec<Span<'static>> = (0..w)
                .map(|x| {
                    let top = px(x, y);
                    let bottom = if y + 1 < h { px(x, y + 1) } else { None };
                    match (top, bottom) {
                        (Some(t), Some(b)) => Span::styled("▀", Style::default().fg(t).bg(b)),
                        (Some(t), None) => Span::styled("▀", Style::default().fg(t)),
                        (None, Some(b)) => Span::styled("▄", Style::default().fg(b)),
                        (None, None) => Span::raw(" "),
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn load_local(base: Option<&Path>, src: &str) -> Result<DynamicImage, String> {
    let path = match base {
        Some(b) => b.join(src),
        None => PathBuf::from(src),
    };
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("não é um arquivo regular".into());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err("imagem grande demais".into());
    }
    decode(&std::fs::read(&path).map_err(|e| e.to_string())?)
}

pub fn is_remote(src: &str) -> bool {
    let s = src.to_ascii_lowercase();
    s.starts_with("http://") || s.starts_with("https://")
}

pub struct ImageStore {
    base_dir: Option<PathBuf>,
    cache: Arc<Mutex<HashMap<String, ImageState>>>,
}

impl ImageStore {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        ImageStore { base_dir, cache: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn get(&self, src: &str) -> ImageState {
        if let Some(s) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(src) {
            return s.clone();
        }
        let state = if is_remote(src) {
            ImageState::Failed("imagem remota: use --remote".into())
        } else {
            match load_local(self.base_dir.as_deref(), src) {
                Ok(img) => ImageState::Ready(Arc::new(img)),
                Err(e) => ImageState::Failed(e),
            }
        };
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(src.to_string(), state.clone());
        state
    }
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test image::`
Expected: 6 testes PASS.

- [ ] **Step 6: Teste de integração no render (falha primeiro)**

Em `src/render.rs`, módulo de testes:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageStore;

    fn text(r: &Rendered) -> String {
        r.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn imagem_local_vira_meio_bloco() {
        let dir = tempfile::tempdir().unwrap();
        let png = {
            let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 10, 10, 255])));
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        };
        std::fs::write(dir.path().join("a.png"), png).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("antes\n\n![logo](a.png)\n\ndepois", &RenderOpts { width: 40, images: Some(&store) });
        let t = text(&r);
        assert!(t.contains('▀'), "{t}");
        assert!(!t.contains("[imagem"), "{t}");
        assert!(t.contains("antes") && t.contains("depois"));
    }

    #[test]
    fn imagem_quebrada_mostra_alt_e_motivo() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let r = render_with("![logo](nao.png)", &RenderOpts { width: 40, images: Some(&store) });
        let t = text(&r);
        assert!(t.contains("[imagem: logo"), "{t}");
        assert!(t.contains('—'), "{t}");
    }

    #[test]
    fn sem_store_mantem_comportamento_antigo() {
        let t = text(&render("![logo](a.png)", 40));
        assert_eq!(t.trim(), "[imagem: logo]");
    }
}
```

Run: `cargo test render::tests`
Expected: erro de compilação (`render_with`, `RenderOpts`).

- [ ] **Step 7: Implementar no `render.rs`**

1. API pública (substitui a assinatura atual, mantendo `render` como atalho):

```rust
use crate::image::{halfblock, ImageState, ImageStore, MAX_IMAGE_COLS, MAX_IMAGE_ROWS};

pub struct RenderOpts<'a> {
    pub width: usize,
    pub images: Option<&'a ImageStore>,
}

pub fn render(md: &str, width: usize) -> Rendered {
    render_with(md, &RenderOpts { width, images: None })
}

pub fn render_with(md: &str, opts: &RenderOpts) -> Rendered {
    // corpo atual de `render`, trocando `width.max(10)` por `opts.width.max(10)`
    // e adicionando `images: opts.images, image: None,` na construção de Renderer
}
```

2. `struct Renderer<'a>` (e `impl<'a> Renderer<'a>`) ganha:

```rust
images: Option<&'a ImageStore>,
image: Option<ImageCtx>,
```
```rust
struct ImageCtx {
    url: String,
    /// Posição em `inline` antes do "[imagem: " de fallback.
    mark: usize,
}
```

3. Em `start`, substituir o braço `Tag::Image { .. }`:

```rust
Tag::Image { dest_url, .. } => {
    self.styles.push(Style::default().add_modifier(Modifier::ITALIC));
    self.image = Some(ImageCtx { url: dest_url.to_string(), mark: self.inline.len() });
    self.inline.push(("[imagem: ".into(), Style::default().fg(DIM)));
}
```

4. Em `end`, substituir `TagEnd::Image`:

```rust
TagEnd::Image => {
    self.styles.pop();
    let dim = Style::default().fg(DIM);
    let Some(ctx) = self.image.take() else { return };
    match self.images.map(|s| s.get(&ctx.url)) {
        Some(ImageState::Ready(img)) => {
            self.inline.truncate(ctx.mark);
            self.flush_inline();
            self.push_image(&img);
        }
        Some(ImageState::Loading) => self.inline.push((" … carregando]".into(), dim)),
        Some(ImageState::Failed(e)) => self.inline.push((format!(" — {e}]"), dim)),
        None => self.inline.push(("]".into(), dim)),
    }
}
```

5. Novo método em `impl Renderer`:

```rust
fn push_image(&mut self, img: &image::DynamicImage) {
    let (first, cont) = self.prefixes();
    let pw: usize = first.iter().map(|s| s.content.width()).sum();
    let cols = self.width.saturating_sub(pw).clamp(1, MAX_IMAGE_COLS);
    for (i, line) in halfblock(img, cols, MAX_IMAGE_ROWS).into_iter().enumerate() {
        let mut v = if i == 0 { first.clone() } else { cont.clone() };
        v.extend(line.spans);
        self.lines.push(Line::from(v));
    }
}
```

6. `main.rs`: `mod image;`; `App` ganha `images: image::ImageStore` criado em `App::new` com `base_dir = path.parent()` (ou `None` para stdin; `Some(".")` se o pai for vazio); `rerender` passa a chamar `render::render_with(&self.md, &render::RenderOpts { width: w, images: Some(&self.images) })`. Modo `--plain` faz o mesmo com uma store própria.

- [ ] **Step 8: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: todos PASS; sem warnings novos.

- [ ] **Step 9: Verificação manual**

Criar `samples/img.md` com `![logo](logo.png)` e um `logo.png` qualquer.
Run: `cargo run --release -- -p samples/img.md`
Expected: bloco colorido de meio-blocos no lugar de `[imagem: …]`.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src samples
git commit -m "feat(imagens): renderização de imagens locais em meio-bloco"
```

---

### Task 3: Imagens por URL (`--remote`)

**Files:**
- Create: `src/net.rs`
- Modify: `Cargo.toml`, `src/image.rs` (`ImageStore` assíncrono), `src/main.rs` (flag, `check_reload` observa a versão do store)

**Interfaces:**
- Consumes: `image::{decode, ImageState, ImageStore, MAX_IMAGE_BYTES, is_remote}` (Task 2), `Cli.remote` (Task 1).
- Produces:
  ```rust
  // net.rs
  pub struct FetchPolicy { pub allow_private: bool, pub max_bytes: u64, pub timeout: Duration, pub max_redirects: u8 }
  impl Default for FetchPolicy
  pub fn is_public_ip(ip: IpAddr) -> bool
  pub fn fetch(url: &str, policy: &FetchPolicy) -> Result<Vec<u8>, String>
  // image.rs
  impl ImageStore {
      pub fn with_remote(self, policy: FetchPolicy, blocking: bool) -> Self;
      pub fn version(&self) -> u64;               // incrementa a cada download concluído
      pub fn get_job(&self, key: &str, run_async: bool, job: impl FnOnce() -> Result<DynamicImage, String> + Send + 'static) -> ImageState;
  }
  ```

- [ ] **Step 1: Dependências**

```toml
ureq = { version = "2", default-features = false, features = ["tls"] }
url = "2"
```

- [ ] **Step 2: Testes que falham em `src/net.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve_once(resp: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(&resp);
            }
        });
        format!("http://127.0.0.1:{port}/img.png")
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn permissive() -> FetchPolicy {
        FetchPolicy { allow_private: true, ..FetchPolicy::default() }
    }

    #[test]
    fn ips_privados_sao_bloqueados() {
        for ip in ["127.0.0.1", "10.0.0.5", "172.16.3.1", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1", "::1", "fc00::1", "fe80::1", "::ffff:127.0.0.1"] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn baixa_corpo_de_servidor_local_quando_permitido() {
        let url = serve_once(ok(b"abc"));
        assert_eq!(fetch(&url, &permissive()).unwrap(), b"abc");
    }

    #[test]
    fn bloqueia_loopback_por_padrao() {
        let url = serve_once(ok(b"abc"));
        assert!(fetch(&url, &FetchPolicy::default()).is_err());
    }

    #[test]
    fn rejeita_corpo_maior_que_o_teto() {
        let url = serve_once(ok(&[0u8; 100]));
        let p = FetchPolicy { max_bytes: 10, ..permissive() };
        assert!(fetch(&url, &p).unwrap_err().contains("grande"));
    }

    #[test]
    fn rejeita_esquemas_nao_http() {
        for u in ["file:///etc/passwd", "ftp://x/y", "javascript:alert(1)", "http://user:pw@host/x"] {
            assert!(fetch(u, &permissive()).is_err(), "{u}");
        }
    }
}
```

- [ ] **Step 3: Rodar e ver falhar**

Run: `cargo test net::`
Expected: erro de compilação.

- [ ] **Step 4: Implementar `src/net.rs`**

```rust
//! Download HTTP(S) restrito: sem rede privada, com teto de bytes e redirecionamento manual.

use std::io::{self, Read};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Duration;

use url::Url;

pub struct FetchPolicy {
    pub allow_private: bool,
    pub max_bytes: u64,
    pub timeout: Duration,
    pub max_redirects: u8,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        FetchPolicy {
            allow_private: false,
            max_bytes: crate::image::MAX_IMAGE_BYTES,
            timeout: Duration::from_secs(8),
            max_redirects: 3,
        }
    }
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || (o[0] == 100 && (64..128).contains(&o[1])))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let seg0 = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00
                || (seg0 & 0xffc0) == 0xfe80)
        }
    }
}

/// Filtra os endereços na hora da resolução; o `ureq` conecta só no que sai daqui,
/// o que fecha a janela de DNS rebinding.
struct PolicyResolver {
    allow_private: bool,
}

impl ureq::Resolver for PolicyResolver {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let addrs: Vec<SocketAddr> = netloc
            .to_socket_addrs()?
            .filter(|a| self.allow_private || is_public_ip(a.ip()))
            .collect();
        if addrs.is_empty() {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "endereço bloqueado (rede privada)"))
        } else {
            Ok(addrs)
        }
    }
}

fn check_url(u: &Url) -> Result<(), String> {
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!("esquema não permitido: {}", u.scheme()));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("URL com credenciais não é permitida".into());
    }
    Ok(())
}

pub fn fetch(url: &str, policy: &FetchPolicy) -> Result<Vec<u8>, String> {
    let mut current = Url::parse(url).map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(policy.timeout)
        .resolver(PolicyResolver { allow_private: policy.allow_private })
        .build();
    for _ in 0..=policy.max_redirects {
        check_url(&current)?;
        let resp = agent.get(current.as_str()).call().map_err(|e| e.to_string())?;
        if (300..400).contains(&resp.status()) {
            let loc = resp.header("location").ok_or("redirecionamento sem Location")?;
            current = current.join(loc).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(len) = resp.header("content-length").and_then(|v| v.parse::<u64>().ok()) {
            if len > policy.max_bytes {
                return Err("resposta grande demais".into());
            }
        }
        let mut buf = Vec::new();
        resp.into_reader()
            .take(policy.max_bytes + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() as u64 > policy.max_bytes {
            return Err("resposta grande demais".into());
        }
        return Ok(buf);
    }
    Err("redirecionamentos demais".into())
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test net::`
Expected: 5 testes PASS.

- [ ] **Step 6: Testes do `ImageStore` assíncrono (falham primeiro)**

Em `src/image.rs`, dentro de `mod tests` acrescentar (reaproveita `png_bytes`):

```rust
use crate::net::FetchPolicy;
use std::io::{Read, Write};
use std::net::TcpListener;

fn serve_png() -> String {
    let body = png_bytes(4, 4);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..2 {
            if let Ok((mut s, _)) = l.accept() {
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        }
    });
    format!("http://127.0.0.1:{port}/a.png")
}

fn local_ok() -> FetchPolicy {
    FetchPolicy { allow_private: true, ..FetchPolicy::default() }
}

#[test]
fn remoto_bloqueante_devolve_pronto() {
    let store = ImageStore::new(None).with_remote(local_ok(), true);
    assert!(matches!(store.get(&serve_png()), ImageState::Ready(_)));
}

#[test]
fn remoto_assincrono_comeca_loading_e_termina_pronto() {
    let store = ImageStore::new(None).with_remote(local_ok(), false);
    let url = serve_png();
    assert!(matches!(store.get(&url), ImageState::Loading));
    for _ in 0..100 {
        if matches!(store.get(&url), ImageState::Ready(_)) {
            assert!(store.version() > 0);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("download não terminou");
}

#[test]
fn limite_de_imagens_remotas() {
    let store = ImageStore::new(None).with_remote(local_ok(), false);
    for i in 0..MAX_REMOTE_IMAGES {
        store.get(&format!("http://127.0.0.1:1/{i}.png"));
    }
    match store.get("http://127.0.0.1:1/extra.png") {
        ImageState::Failed(m) => assert!(m.contains("limite")),
        _ => panic!("deveria estourar o limite"),
    }
}
```

Run: `cargo test image::`
Expected: erro de compilação (`with_remote`, `version`, `MAX_REMOTE_IMAGES`).

- [ ] **Step 7: Implementar no `ImageStore`**

Em `src/image.rs`: adicionar `use std::sync::atomic::{AtomicU64, Ordering};` e `use crate::net::{self, FetchPolicy};`, a constante `pub const MAX_REMOTE_IMAGES: usize = 32;` e reescrever a struct e o `get`:

```rust
pub struct ImageStore {
    base_dir: Option<PathBuf>,
    cache: Arc<Mutex<HashMap<String, ImageState>>>,
    version: Arc<AtomicU64>,
    remote: Option<(Arc<FetchPolicy>, bool)>, // (política, bloqueante)
}

impl ImageStore {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        ImageStore {
            base_dir,
            cache: Arc::new(Mutex::new(HashMap::new())),
            version: Arc::new(AtomicU64::new(0)),
            remote: None,
        }
    }

    pub fn with_remote(mut self, policy: FetchPolicy, blocking: bool) -> Self {
        self.remote = Some((Arc::new(policy), blocking));
        self
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, ImageState>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Executa `job` (agora ou em thread) e guarda o resultado sob `key`.
    pub fn get_job(
        &self,
        key: &str,
        run_async: bool,
        job: impl FnOnce() -> Result<DynamicImage, String> + Send + 'static,
    ) -> ImageState {
        if let Some(s) = self.lock().get(key) {
            return s.clone();
        }
        let to_state = |r: Result<DynamicImage, String>| match r {
            Ok(img) => ImageState::Ready(Arc::new(img)),
            Err(e) => ImageState::Failed(e),
        };
        if !run_async {
            let st = to_state(job());
            self.lock().insert(key.to_string(), st.clone());
            return st;
        }
        self.lock().insert(key.to_string(), ImageState::Loading);
        let (cache, version, key) = (Arc::clone(&self.cache), Arc::clone(&self.version), key.to_string());
        std::thread::spawn(move || {
            let st = to_state(job());
            cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key, st);
            version.fetch_add(1, Ordering::Relaxed);
        });
        ImageState::Loading
    }

    pub fn get(&self, src: &str) -> ImageState {
        if !is_remote(src) {
            let base = self.base_dir.clone();
            let s = src.to_string();
            return self.get_job(src, false, move || load_local(base.as_deref(), &s));
        }
        let Some((policy, blocking)) = &self.remote else {
            return ImageState::Failed("imagem remota: use --remote".into());
        };
        let remote_count = self.lock().keys().filter(|k| is_remote(k)).count();
        if remote_count >= MAX_REMOTE_IMAGES && !self.lock().contains_key(src) {
            return ImageState::Failed("limite de imagens remotas atingido".into());
        }
        let (policy, url) = (Arc::clone(policy), src.to_string());
        self.get_job(src, !*blocking, move || {
            decode(&net::fetch(&url, &policy)?)
        })
    }
}
```

(`get_job` chaves `mermaid:…` da Task 4 não começam com `http`, então não contam no limite.)

Em `main.rs`: `mod net;`; ao construir a store, `if cli.remote { store = store.with_remote(FetchPolicy::default(), plain_mode) }`; em `App` guardar `seen_version: u64` e, em `check_reload` (chamado a cada 250 ms), `let v = self.images.version(); if v != self.seen_version { self.seen_version = v; self.dirty = true; }` (esse bloco fica antes do `let Source::File(p) = … else { return }` para valer também para stdin).

- [ ] **Step 8: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS; sem warnings novos.

- [ ] **Step 9: Verificação manual**

Run: `printf '![x](https://www.rust-lang.org/static/images/rust-logo-blk.svg)\n![y](https://raw.githubusercontent.com/rust-lang/rust-artwork/master/logo/rust-logo-128x128.png)' | cargo run --release -- -p --remote`
Expected: o PNG aparece; o SVG mostra `[imagem: x — …]` (SVG não suportado, falha limpa). Sem `--remote`: `[imagem: y — imagem remota: use --remote]`.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -m "feat(imagens): imagens por URL com --remote, política anti-SSRF e download em segundo plano"
```

---

### Task 4: Diagramas Mermaid (`--mermaid`)

**Files:**
- Create: `src/mermaid.rs`
- Modify: `src/image.rs` (`ImageStore::get_mermaid`), `src/render.rs` (`render_code`), `src/main.rs`, `Cargo.toml` (`tempfile` também em `[dependencies]`)

**Interfaces:**
- Consumes: `ImageStore::get_job`, `decode` (Task 3/2).
- Produces:
  ```rust
  // mermaid.rs
  pub const MAX_MERMAID_BYTES: usize = 64 * 1024;
  pub fn args(input: &Path, output: &Path) -> Vec<OsString>
  pub fn render_png(source: &str, bin: &str, timeout: Duration) -> Result<Vec<u8>, String>
  // image.rs
  impl ImageStore { pub fn with_mermaid(self, bin: String, blocking: bool) -> Self; pub fn mermaid_enabled(&self) -> bool; pub fn get_mermaid(&self, source: &str) -> ImageState }
  ```

Contexto: não há renderizador Mermaid maduro em Rust puro; o caminho pragmático é delegar ao `mmdc` (Node + Chromium headless) e exibir o PNG pela mesma via das imagens. Sem `--mermaid` ou sem `mmdc` instalado, o bloco continua sendo mostrado como código (fonte), com o motivo no cabeçalho.

- [ ] **Step 1: Testes que falham em `src/mermaid.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn argumentos_do_mmdc() {
        let a = args(Path::new("/t/in.mmd"), Path::new("/t/out.png"));
        let a: Vec<String> = a.iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(a, ["-i", "/t/in.mmd", "-o", "/t/out.png", "-b", "transparent", "-w", "1200", "-q"]);
    }

    #[test]
    fn binario_inexistente_da_erro_claro() {
        let e = render_png("graph TD; A-->B", "/nao/existe/mmdc", Duration::from_secs(2)).unwrap_err();
        assert!(e.contains("mmdc"), "{e}");
    }

    #[test]
    fn fonte_grande_demais_e_recusada() {
        let src = "A".repeat(MAX_MERMAID_BYTES + 1);
        assert!(render_png(&src, "mmdc", Duration::from_secs(2)).unwrap_err().contains("grande"));
    }

    #[cfg(unix)]
    fn fake(dir: &Path, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("fake-mmdc");
        std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    #[test]
    fn devolve_png_gerado_pelo_binario() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("fixture.png");
        std::fs::write(&png, b"PNGDATA").unwrap();
        // $4 é o caminho de saída (-i in -o out ...)
        let bin = fake(dir.path(), &format!("cp {} \"$4\"", png.display()));
        assert_eq!(render_png("graph TD; A-->B", &bin, Duration::from_secs(5)).unwrap(), b"PNGDATA");
    }

    #[cfg(unix)]
    #[test]
    fn estoura_timeout_e_mata_o_processo() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "sleep 10");
        let t = Instant::now();
        let e = render_png("graph TD; A-->B", &bin, Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("tempo"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn saida_de_erro_do_binario_vira_mensagem() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake(dir.path(), "echo 'Parse error on line 1' >&2; exit 1");
        let e = render_png("x", &bin, Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("Parse error"), "{e}");
    }
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test mermaid::`
Expected: erro de compilação.

- [ ] **Step 3: Implementar `src/mermaid.rs`**

```rust
//! Renderiza Mermaid chamando o `mmdc` (mermaid-cli) como subprocesso.

use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_MERMAID_BYTES: usize = 64 * 1024;
pub const MMDC_TIMEOUT: Duration = Duration::from_secs(20);

pub fn args(input: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "-i".into(), input.into(), "-o".into(), output.into(),
        "-b".into(), "transparent".into(), "-w".into(), "1200".into(), "-q".into(),
    ]
}

pub fn render_png(source: &str, bin: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    if source.len() > MAX_MERMAID_BYTES {
        return Err("diagrama grande demais".into());
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (input, output) = (dir.path().join("in.mmd"), dir.path().join("out.png"));
    std::fs::write(&input, source).map_err(|e| e.to_string())?;

    let mut child = Command::new(bin)
        .args(args(&input, &output))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não consegui executar mmdc ({bin}): {e}"))?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("tempo esgotado no mmdc".into());
            }
            None => std::thread::sleep(Duration::from_millis(30)),
        }
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.take(2048).read_to_string(&mut err);
        }
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("falha desconhecida");
        return Err(format!("mmdc: {}", first.trim()));
    }
    std::fs::read(&output).map_err(|e| format!("mmdc não gerou a imagem: {e}"))
}
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test mermaid::`
Expected: 6 testes PASS (3 só em unix).

- [ ] **Step 5: Teste de integração no render (falha primeiro)**

Em `src/render.rs`, `mod tests`:

```rust
#[cfg(unix)]
#[test]
fn bloco_mermaid_vira_imagem_quando_habilitado() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("f.png");
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])));
    img.save(&png).unwrap();
    let bin = dir.path().join("mmdc");
    std::fs::write(&bin, format!("#!/bin/sh\ncp {} \"$4\"\n", png.display())).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let store = ImageStore::new(None).with_mermaid(bin.to_string_lossy().into_owned(), true);
    let r = render_with("```mermaid\ngraph TD; A-->B\n```", &RenderOpts { width: 40, images: Some(&store) });
    assert!(text(&r).contains('▀'));
}

#[test]
fn mermaid_sem_flag_mostra_codigo() {
    let r = render("```mermaid\ngraph TD; A-->B\n```", 40);
    let t = text(&r);
    assert!(t.contains("mermaid") && t.contains("A-->B"), "{t}");
}

#[test]
fn mermaid_com_falha_mostra_codigo_e_motivo() {
    let store = ImageStore::new(None).with_mermaid("/nao/existe/mmdc".into(), true);
    let r = render_with("```mermaid\ngraph TD; A-->B\n```", &RenderOpts { width: 60, images: Some(&store) });
    let t = text(&r);
    assert!(t.contains("A-->B") && t.contains("erro"), "{t}");
}
```

Run: `cargo test render::tests::` → falha de compilação (`with_mermaid`).

- [ ] **Step 6: Implementar**

`image.rs` (campos `mermaid: Option<(String, bool)>` no `ImageStore`, inicializado com `None`):

```rust
pub fn with_mermaid(mut self, bin: String, blocking: bool) -> Self {
    self.mermaid = Some((bin, blocking));
    self
}

pub fn mermaid_enabled(&self) -> bool {
    self.mermaid.is_some()
}

pub fn get_mermaid(&self, source: &str) -> ImageState {
    use std::hash::{Hash, Hasher};
    let Some((bin, blocking)) = &self.mermaid else {
        return ImageState::Failed("mermaid desativado: use --mermaid".into());
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    let key = format!("mermaid:{:016x}", h.finish());
    let (bin, src) = (bin.clone(), source.to_string());
    self.get_job(&key, !*blocking, move || {
        decode(&crate::mermaid::render_png(&src, &bin, crate::mermaid::MMDC_TIMEOUT)?)
    })
}
```

`render.rs`, no início de `render_code`, logo após ler `lang` e `code`:

```rust
let mut label = lang.clone();
if lang == "mermaid" {
    if let Some(store) = self.images.filter(|s| s.mermaid_enabled()) {
        match store.get_mermaid(&code) {
            ImageState::Ready(img) => {
                self.push_image(&img);
                return;
            }
            ImageState::Loading => label = "mermaid (renderizando…)".into(),
            ImageState::Failed(e) => label = format!("mermaid (erro: {e})"),
        }
    }
}
```

e trocar o cabeçalho para usar `label` (`if label.is_empty() { "╭─".into() } else { format!("╭─ {label}") }`). Nota: `lang` continua selecionando a sintaxe do `syntect`; `label` só vai para o cabeçalho (passar o texto por `sanitize` — mensagens do `mmdc` vêm de processo externo).

`main.rs`: `if cli.mermaid { store = store.with_mermaid(std::env::var("MDV_MMDC").unwrap_or_else(|_| "mmdc".into()), plain_mode) }`.

- [ ] **Step 7: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS.

- [ ] **Step 8: Verificação manual**

Com `mmdc` instalado (`npm i -g @mermaid-js/mermaid-cli`):
Run: `printf '# d\n\n```mermaid\ngraph TD; A[Início]-->B[Fim]\n```\n' | cargo run --release -- -p --mermaid`
Expected: diagrama em meio-bloco. Sem `--mermaid`: bloco de código com cabeçalho `╭─ mermaid`. Sem `mmdc`: cabeçalho `mermaid (erro: não consegui executar mmdc …)`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -m "feat(mermaid): renderiza blocos mermaid via mmdc com --mermaid"
```

---

### Task 5: Compatibilidade com Markdown de LLMs/agentes

**Files:**
- Modify: `src/render.rs` (`render_with`: opções do parser; `start`/`end`/`handle`; `style`)

**Interfaces:**
- Consumes: `Renderer::render_code`, `push_blank`, `quote_prefix` (existentes).
- Produces: `fn think_tags(line: &str) -> Option<ThinkLine>` com `struct ThinkLine { opens: bool, closes: bool, rest: String }` (privados, testáveis).

Construções cobertas: front matter YAML, alertas GFM (`> [!NOTE]`), notas de rodapé, `<details>/<summary>`, `<think>`/`<thinking>`, cerca de código sem fechamento (já tratada pelo parser; entra como teste de regressão).

- [ ] **Step 1: Testes que falham**

Em `src/render.rs`, `mod tests`:

```rust
#[test]
fn front_matter_vira_bloco_yaml() {
    let t = text(&render("---\ntitle: Plano\ntags: [a]\n---\n\n# Título", 60));
    assert!(t.contains("yaml") && t.contains("title: Plano"), "{t}");
    assert!(!t.contains("---"), "{t}");
}

#[test]
fn alerta_gfm_mostra_rotulo_em_portugues() {
    let t = text(&render("> [!WARNING]\n> cuidado aqui", 60));
    assert!(t.contains("Aviso") && t.contains("cuidado aqui"), "{t}");
    assert!(!t.contains("[!WARNING]"), "{t}");
}

#[test]
fn nota_de_rodape_aparece_no_texto_e_na_definicao() {
    let t = text(&render("Fato[^1].\n\n[^1]: Fonte aqui.", 60));
    assert!(t.contains("[^1]") && t.contains("Fonte aqui"), "{t}");
}

#[test]
fn details_e_summary_viram_titulo() {
    let t = text(&render("<details>\n<summary>Ver mais</summary>\n\nconteúdo\n\n</details>", 60));
    assert!(t.contains("▸ Ver mais") && t.contains("conteúdo"), "{t}");
    assert!(!t.contains("<details>") && !t.contains("<summary>"), "{t}");
}

#[test]
fn think_vira_bloco_rotulado() {
    let t = text(&render("<think>\n\nplano interno\n\n</think>\n\nresposta final", 60));
    assert!(t.contains("raciocínio") && t.contains("plano interno") && t.contains("resposta final"), "{t}");
    assert!(!t.contains("<think>"), "{t}");
}

#[test]
fn think_na_mesma_linha() {
    let t = text(&render("<think>só isto</think>\n\nok", 60));
    assert!(t.contains("só isto") && !t.contains("</think>"), "{t}");
}

#[test]
fn think_tags_parse() {
    let l = think_tags("<thinking>abc").unwrap();
    assert!(l.opens && !l.closes && l.rest == "abc");
    assert!(think_tags("<div>x</div>").is_none());
}

#[test]
fn cerca_sem_fechar_nao_quebra() {
    let t = text(&render("```rust\nfn main() {", 40));
    assert!(t.contains("fn main"), "{t}");
}
```

Run: `cargo test render::tests::`
Expected: FAIL (front matter e alertas ainda não tratados; `think_tags` indefinida).

- [ ] **Step 2: Opções do parser**

Em `render_with`:

```rust
let opts = Options::ENABLE_TABLES
    | Options::ENABLE_STRIKETHROUGH
    | Options::ENABLE_TASKLISTS
    | Options::ENABLE_FOOTNOTES
    | Options::ENABLE_GFM
    | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
```

(`ENABLE_MATH` fica de fora de propósito: textos de LLM com `$5 e $10` seriam lidos como fórmula.)

- [ ] **Step 3: Front matter, alertas e notas de rodapé**

`use pulldown_cmark::BlockQuoteKind;` e, em `start`:

```rust
Tag::MetadataBlock(_) => {
    self.flush_inline();
    self.code = Some("yaml".to_string());
    self.code_buf.clear();
}
Tag::BlockQuote(kind) => {
    self.flush_inline();
    self.quote_depth += 1;
    self.styles.push(Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC));
    if let Some(k) = kind {
        let (label, color) = match k {
            BlockQuoteKind::Note => ("ℹ Nota", H2),
            BlockQuoteKind::Tip => ("💡 Dica", H3),
            BlockQuoteKind::Important => ("❗ Importante", H1),
            BlockQuoteKind::Warning => ("⚠ Aviso", Color::Rgb(240, 192, 64)),
            BlockQuoteKind::Caution => ("⛔ Cuidado", Color::Rgb(248, 81, 73)),
        };
        let mut v = self.quote_prefix();
        v.push(Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)));
        self.lines.push(Line::from(v));
    }
}
Tag::FootnoteDefinition(label) => {
    self.flush_inline();
    self.inline.push((format!("[^{label}]: "), Style::default().fg(DIM)));
}
```

(substitui o braço atual de `Tag::BlockQuote(_)`); em `end`:

```rust
TagEnd::MetadataBlock(_) => {
    self.render_code();
    self.code = None;
    self.push_blank();
}
TagEnd::FootnoteDefinition => self.flush_inline(),
```

e em `handle`:

```rust
Event::FootnoteReference(label) => {
    self.inline.push((format!("[^{label}]"), Style::default().fg(DIM)));
}
```

- [ ] **Step 4: `<details>`, `<summary>` e `<think>`**

Campo `think: usize` em `Renderer` (inicial 0). `style()` passa a começar assim:

```rust
fn style(&self) -> Style {
    let base = if self.think > 0 {
        Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC)
    } else {
        Style::default()
    };
    self.styles.iter().fold(base, |a, s| a.patch(*s))
}
```

Funções livres:

```rust
struct ThinkLine {
    opens: bool,
    closes: bool,
    rest: String,
}

fn think_tags(line: &str) -> Option<ThinkLine> {
    const OPEN: [&str; 2] = ["<think>", "<thinking>"];
    const CLOSE: [&str; 2] = ["</think>", "</thinking>"];
    let opens = OPEN.iter().any(|t| line.contains(t));
    let closes = CLOSE.iter().any(|t| line.contains(t));
    if !opens && !closes {
        return None;
    }
    let mut rest = line.to_string();
    for t in OPEN.iter().chain(CLOSE.iter()) {
        rest = rest.replace(t, "");
    }
    Some(ThinkLine { opens, closes, rest: rest.trim().to_string() })
}

fn summary_text(line: &str) -> Option<&str> {
    let l = line.trim();
    l.strip_prefix("<summary>")?.strip_suffix("</summary>").map(str::trim)
}
```

Substituir o braço `Event::Html(h)` em `handle`:

```rust
Event::Html(h) => {
    self.flush_inline();
    let dim = Style::default().fg(DIM);
    for l in h.lines() {
        let t = l.trim();
        if t.starts_with("<details") || t == "</details>" {
            continue;
        }
        if let Some(s) = summary_text(t) {
            self.lines.push(Line::from(Span::styled(
                format!("▸ {s}"),
                Style::default().fg(H2).add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(tl) = think_tags(t) {
            if tl.opens {
                self.lines.push(Line::from(Span::styled("💭 raciocínio", dim.add_modifier(Modifier::ITALIC))));
            }
            if !tl.rest.is_empty() {
                self.lines.push(Line::from(Span::styled(
                    tl.rest,
                    Style::default().fg(QUOTE).add_modifier(Modifier::ITALIC),
                )));
            }
            if tl.opens && !tl.closes {
                self.think += 1;
            } else if tl.closes && !tl.opens {
                self.think = self.think.saturating_sub(1);
            }
            continue;
        }
        self.lines.push(Line::from(Span::styled(l.to_string(), dim)));
    }
}
```

- [ ] **Step 5: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS.

- [ ] **Step 6: Verificação manual**

Criar `samples/llm.md` com front matter, um alerta, nota de rodapé, `<details>` e `<think>`.
Run: `cargo run --release -- -p samples/llm.md`
Expected: nenhuma tag HTML crua das construções acima; bloco `yaml` no topo.

- [ ] **Step 7: Commit**

```bash
git add src samples
git commit -m "feat(render): front matter, alertas GFM, notas de rodapé, details e think de LLMs"
```

---

### Task 6: Fonte crua (`m`) e sumário (`--toc`)

**Files:**
- Modify: `src/render.rs` (`render_code_only`, `Rendered.outline`, `toc`), `src/main.rs` (`App.raw`, tecla `m`, `--toc`, status/ajuda)

**Interfaces:**
- Produces:
  ```rust
  pub fn render_code_only(code: &str, lang: &str, width: usize) -> Rendered
  pub struct OutlineEntry { pub line: usize, pub level: u8, pub title: String }
  // Rendered ganha: pub outline: Vec<OutlineEntry>
  pub fn toc(r: &Rendered) -> String
  pub fn error_rendered(msg: &str) -> Rendered   // uma linha vermelha; usado por data.rs
  ```

- [ ] **Step 1: Testes que falham**

```rust
#[test]
fn fonte_crua_mostra_markdown_literal() {
    let t = text(&render_code_only("# a\n**b**\n", "md", 40));
    assert!(t.contains("# a") && t.contains("**b**") && t.contains("md"), "{t}");
}

#[test]
fn toc_indenta_por_nivel() {
    let r = render("# A\n\n## B\n\n### C\n\n## D", 40);
    assert_eq!(toc(&r), "A\n  B\n    C\n  D\n");
}

#[test]
fn toc_sem_titulos_e_vazio() {
    assert_eq!(toc(&render("só texto", 40)), "");
}

#[test]
fn erro_renderizado_tem_uma_linha() {
    assert_eq!(error_rendered("falhou").lines.len(), 1);
}
```

Run: `cargo test render::tests::`
Expected: erro de compilação.

- [ ] **Step 2: Implementar em `render.rs`**

`Rendered` ganha `pub outline: Vec<OutlineEntry>` (derive `Default` continua valendo); `finish_heading` registra, antes de `wrap`, `let title: String = toks.iter().map(|(t, _)| t.as_str()).collect();` e `self.outline.push(OutlineEntry { line: self.lines.len(), level: level as u8, title })` (`HeadingLevel as u8` dá 1..=6); `Renderer` ganha `outline: Vec<OutlineEntry>` e `render_with` o devolve no `Rendered`.

```rust
pub fn toc(r: &Rendered) -> String {
    r.outline
        .iter()
        .map(|e| format!("{}{}\n", "  ".repeat(e.level.saturating_sub(1) as usize), e.title))
        .collect()
}

pub fn render_code_only(code: &str, lang: &str, width: usize) -> Rendered {
    let mut r = Renderer::new(width, None);
    r.code = Some(lang.to_string());
    r.code_buf = code.to_string();
    r.render_code();
    Rendered { lines: r.lines, headings: Vec::new(), outline: Vec::new() }
}

pub fn error_rendered(msg: &str) -> Rendered {
    let st = Style::default().fg(Color::Rgb(248, 81, 73)).add_modifier(Modifier::BOLD);
    Rendered {
        lines: vec![Line::from(Span::styled(sanitize(msg).into_owned(), st))],
        headings: Vec::new(),
        outline: Vec::new(),
    }
}
```

Extrair de `render_with` o literal de construção do `Renderer` para `Renderer::new(width: usize, images: Option<&'a ImageStore>) -> Self` (reaproveitado aqui). `sanitize` é o ponto único de saneamento da Fase 1, item 1.

- [ ] **Step 3: `main.rs`**

- `App.raw: bool`; tecla `KeyCode::Char('m') => { self.raw = !self.raw; self.dirty = true; }`.
- `rerender`: `if self.raw { render::render_code_only(&self.md, "md", w) } else { render_with(...) }`.
- `status_line`: acrescentar `  [fonte]` quando `raw`; `HELP` e a legenda da barra ganham `m  alterna fonte crua`.
- Em `main`, antes do `if plain`: `if cli.toc { print!("{}", render::toc(&render::render(&md, w))); return Ok(()); }` (para `Mode::Markdown`).

- [ ] **Step 4: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS.

- [ ] **Step 5: Verificação manual**

Run: `cargo run --release -- --toc sample.md`
Expected: lista indentada dos títulos. Na TUI, `m` alterna entre renderizado e fonte.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat: tecla m (fonte crua) e --toc"
```

---

### Task 7: Streaming de stdin (`--follow`) e `--latest <pasta>`

**Files:**
- Create: `src/stream.rs`
- Modify: `src/main.rs` (`App`: `stream`, `stick_bottom`, `latest_dir`; `run`: intervalo de poll)

**Interfaces:**
- Produces:
  ```rust
  pub struct Utf8Accum { .. }
  impl Utf8Accum { pub fn new() -> Self; pub fn push(&mut self, bytes: &[u8]) -> String }
  pub fn spawn_stdin_reader(max_bytes: usize) -> std::sync::mpsc::Receiver<String>
  pub fn newest_md(dir: &Path) -> Option<PathBuf>
  ```

- [ ] **Step 1: Testes que falham**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    #[test]
    fn utf8_dividido_entre_chunks() {
        let mut a = Utf8Accum::new();
        let bytes = "é".as_bytes(); // 2 bytes
        assert_eq!(a.push(&bytes[..1]), "");
        assert_eq!(a.push(&bytes[1..]), "é");
    }

    #[test]
    fn utf8_invalido_vira_substituto_sem_travar() {
        let mut a = Utf8Accum::new();
        assert_eq!(a.push(b"a\xFFb"), "a\u{FFFD}b");
    }

    #[test]
    fn newest_md_escolhe_o_mais_recente_e_ignora_outros() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("velho.md");
        let new = d.path().join("novo.md");
        let other = d.path().join("maior.txt");
        for p in [&old, &new, &other] {
            std::fs::write(p, "x").unwrap();
        }
        let now = SystemTime::now();
        File::options().write(true).open(&old).unwrap().set_modified(now - Duration::from_secs(100)).unwrap();
        File::options().write(true).open(&new).unwrap().set_modified(now - Duration::from_secs(10)).unwrap();
        File::options().write(true).open(&other).unwrap().set_modified(now).unwrap();
        assert_eq!(newest_md(d.path()), Some(new));
    }

    #[test]
    fn newest_md_pasta_vazia() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(newest_md(d.path()), None);
    }
}
```

Run: `cargo test stream::`
Expected: erro de compilação.

- [ ] **Step 2: Implementar `src/stream.rs`**

```rust
//! Entrada incremental (stdin em streaming) e descoberta do `.md` mais recente.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

pub struct Utf8Accum {
    pending: Vec<u8>,
}

impl Utf8Accum {
    pub fn new() -> Self {
        Utf8Accum { pending: Vec::new() }
    }

    /// Devolve o texto completo disponível; sobra de até 3 bytes fica para o próximo `push`.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    return out;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap_or(""));
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + n);
                        }
                        None => {
                            self.pending.drain(..valid);
                            return out;
                        }
                    }
                }
            }
        }
    }
}

/// Lê stdin em outra thread e envia os pedaços de texto; para ao atingir `max_bytes`.
pub fn spawn_stdin_reader(max_bytes: usize) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let (mut buf, mut acc, mut total) = ([0u8; 8192], Utf8Accum::new(), 0usize);
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 {
                break;
            }
            total += n;
            let chunk = acc.push(&buf[..n.min(max_bytes.saturating_sub(total - n))]);
            if !chunk.is_empty() && tx.send(chunk).is_err() {
                break;
            }
            if total >= max_bytes {
                break;
            }
        }
    });
    rx
}

pub fn newest_md(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("md") || x.eq_ignore_ascii_case("markdown"))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(m, _)| *m)
        .map(|(_, p)| p)
}
```

- [ ] **Step 3: Rodar e ver passar**

Run: `cargo test stream::`
Expected: 4 testes PASS.

- [ ] **Step 4: Integrar no `App`**

- `--follow` (só vale para stdin; com arquivo, ignorado porque o live reload já existe): em `main`, se `cli.follow` e `Source::Stdin`, não ler tudo antes; criar `App` com `md` vazio e `stream: Some(rx)`.
- `App::drain_stream(&mut self)`: no tick, `while let Ok(chunk) = rx.try_recv() { self.stick_bottom |= self.scroll >= self.max_scroll(); self.md.push_str(&chunk); got = true; }`; se `got` → `self.dirty = true`. Depois de `rerender`, se `stick_bottom` → `self.scroll_to(usize::MAX)` e zera `stick_bottom`.
- `run`: `event::poll(Duration::from_millis(if app.stream.is_some() { 80 } else { 250 }))` e chamar `app.drain_stream()` no ramo de timeout, junto de `check_reload`.
- `--latest <pasta>`: `cli.path` é a pasta; resolve `newest_md(dir)` na partida (erro em português se não houver `.md`); `App.latest_dir = Some(dir)`. Em `check_reload`, antes de comparar `mtime`: `if let Some(d) = &self.latest_dir { if let Some(n) = stream::newest_md(d) { if n != current_path { trocar source, name, forçar leitura; scroll_to(0) } } }`.
- Modo `--plain` com `--follow`: ignorar (lê tudo, como hoje).

- [ ] **Step 5: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS.

- [ ] **Step 6: Verificação manual**

Run: `(echo '# um'; sleep 1; echo; echo 'dois'; sleep 1; echo 'três') | cargo run --release -- --follow`
Expected: o conteúdo cresce ao vivo e a tela acompanha o final.
Run: `mkdir -p /tmp/ag && echo '# a' > /tmp/ag/a.md && cargo run --release -- --latest /tmp/ag` e, noutro terminal, `echo '# b' > /tmp/ag/b.md`
Expected: a TUI troca para `b.md`.

- [ ] **Step 7: Commit**

```bash
git add src
git commit -m "feat: --follow para stdin em streaming e --latest para acompanhar a pasta"
```

---

### Task 8: Visualizador JSON (`mdv json`)

**Files:**
- Create: `src/data.rs`
- Modify: `Cargo.toml`, `src/main.rs` (`mod data;`, despacho por `cli.mode`), `src/cli.rs` (nenhuma mudança; já entrega `json_path`)

**Interfaces:**
- Consumes: `render::{render_code_only, error_rendered}` (Task 6), `Cli.json_path`.
- Produces:
  ```rust
  pub const MAX_DATA_BYTES: usize = 5 * 1024 * 1024;
  pub fn parse_json(text: &str) -> Result<serde_json::Value, String>   // JSON ou JSONL
  pub fn select<'a>(v: &'a Value, path: &str) -> Result<&'a Value, String>
  pub fn describe(v: &Value) -> String
  pub fn render_json(text: &str, path: Option<&str>, width: usize) -> Rendered
  ```

- [ ] **Step 1: Dependências**

```toml
serde_json = { version = "1", features = ["preserve_order"] }
```

- [ ] **Step 2: Testes que falham**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(r: &crate::render::Rendered) -> String {
        r.lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn select_navega_chaves_e_indices() {
        let v = json!({"a": {"b": [10, {"c": "x"}]}});
        assert_eq!(select(&v, ".a.b[1].c").unwrap(), &json!("x"));
        assert_eq!(select(&v, ".a.b[0]").unwrap(), &json!(10));
        assert_eq!(select(&v, "").unwrap(), &v);
        assert_eq!(select(&v, ".").unwrap(), &v);
    }

    #[test]
    fn select_erros_claros() {
        let v = json!({"a": [1]});
        assert!(select(&v, ".x").unwrap_err().contains("'x'"));
        assert!(select(&v, ".a[5]").unwrap_err().contains("5"));
        assert!(select(&v, ".a[").is_err());
        assert!(select(&v, "a").is_err());
    }

    #[test]
    fn jsonl_vira_lista() {
        let v = parse_json("{\"a\":1}\n{\"a\":2}\n").unwrap();
        assert_eq!(v, json!([{"a": 1}, {"a": 2}]));
    }

    #[test]
    fn json_invalido_informa_linha_e_coluna() {
        let e = parse_json("{\n  \"a\": ,\n}").unwrap_err();
        assert!(e.contains("linha 2"), "{e}");
    }

    #[test]
    fn describe_resume_tipo_e_tamanho() {
        assert_eq!(describe(&json!({"a": 1, "b": 2})), "objeto · 2 chaves");
        assert_eq!(describe(&json!([1, 2, 3])), "lista · 3 itens");
        assert_eq!(describe(&json!("x")), "texto");
        assert_eq!(describe(&json!(null)), "nulo");
    }

    #[test]
    fn render_json_mostra_resumo_e_conteudo_indentado() {
        let t = text(&render_json("{\"nome\":\"mdv\",\"v\":[1,2]}", None, 60));
        assert!(t.contains("objeto · 2 chaves"), "{t}");
        assert!(t.contains("\"nome\": \"mdv\""), "{t}");
    }

    #[test]
    fn render_json_com_caminho_mostra_so_o_trecho() {
        let t = text(&render_json("{\"a\":{\"b\":1},\"c\":2}", Some(".a"), 60));
        assert!(t.contains("\"b\": 1") && !t.contains("\"c\""), "{t}");
    }

    #[test]
    fn render_json_erro_vira_linha_de_erro() {
        let r = render_json("{", None, 60);
        assert_eq!(r.lines.len(), 1);
        assert!(text(&r).contains("JSON"));
    }

    #[test]
    fn controle_em_string_nao_vaza_para_o_terminal() {
        let r = render_json("{\"x\":\"\\u001b]0;pwn\\u0007\"}", None, 60);
        let ansi = crate::render::to_ansi(&r.lines, false);
        assert!(!ansi.contains('\x1b'), "{ansi:?}");
    }

    #[test]
    fn entrada_grande_e_recusada() {
        let big = format!("[{}]", "1,".repeat(MAX_DATA_BYTES));
        assert!(text(&render_json(&big, None, 60)).contains("grande"));
    }
}
```

Run: `cargo test data::`
Expected: erro de compilação.

- [ ] **Step 3: Implementar `src/data.rs` (parte JSON)**

```rust
//! Visualização de dados estruturados (JSON/JSONL, CSV/TSV) reaproveitando o renderizador.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::render::{error_rendered, render_code_only, Rendered};

pub const MAX_DATA_BYTES: usize = 5 * 1024 * 1024;

pub fn parse_json(text: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(v),
        Err(e) => {
            // JSONL: várias linhas, cada uma um valor.
            let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.len() > 1 {
                let items: Result<Vec<Value>, _> = lines.iter().map(|l| serde_json::from_str(l)).collect();
                if let Ok(items) = items {
                    return Ok(Value::Array(items));
                }
            }
            Err(format!("JSON inválido: {e} (linha {}, coluna {})", e.line(), e.column()))
        }
    }
}

pub fn select<'a>(v: &'a Value, path: &str) -> Result<&'a Value, String> {
    let mut cur = v;
    let mut rest = path.trim();
    if rest.is_empty() || rest == "." {
        return Ok(cur);
    }
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '[']).unwrap_or(r.len());
            let key = &r[..end];
            if key.is_empty() {
                return Err("chave vazia no caminho".into());
            }
            cur = cur.get(key).ok_or_else(|| format!("a chave '{key}' não existe"))?;
            rest = &r[end..];
        } else if let Some(r) = rest.strip_prefix('[') {
            let close = r.find(']').ok_or("falta ']' no caminho")?;
            let idx: usize = r[..close].parse().map_err(|_| format!("índice inválido: {}", &r[..close]))?;
            cur = cur.get(idx).ok_or_else(|| format!("o índice {idx} não existe"))?;
            rest = &r[close + 1..];
        } else {
            return Err(format!("caminho inválido perto de '{rest}' (use .chave e [n])"));
        }
    }
    Ok(cur)
}

pub fn describe(v: &Value) -> String {
    match v {
        Value::Object(m) => format!("objeto · {} chaves", m.len()),
        Value::Array(a) => format!("lista · {} itens", a.len()),
        Value::String(_) => "texto".into(),
        Value::Number(_) => "número".into(),
        Value::Bool(_) => "booleano".into(),
        Value::Null => "nulo".into(),
    }
}

pub fn render_json(text: &str, path: Option<&str>, width: usize) -> Rendered {
    if text.len() > MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo JSON (limite de 5 MiB)");
    }
    let root = match parse_json(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let sel = match select(&root, path.unwrap_or("")) {
        Ok(v) => v,
        Err(e) => return error_rendered(&e),
    };
    let pretty = serde_json::to_string_pretty(sel).unwrap_or_default();
    let mut r = render_code_only(&pretty, "json", width);
    let head = Line::from(Span::styled(
        describe(sel),
        Style::default().fg(crate::render::DIM).add_modifier(Modifier::ITALIC),
    ));
    r.lines.insert(0, head);
    r
}
```

(tornar `pub(crate) const DIM` em `render.rs`.)

- [ ] **Step 4: Rodar e ver passar**

Run: `cargo test data::`
Expected: 10 testes PASS. Se `controle_em_string_nao_vaza_para_o_terminal` falhar, o saneamento da Fase 1 não cobre `render_code` — corrigir lá (ponto único), não aqui.

- [ ] **Step 5: Despacho em `main.rs`**

Criar `fn render_source(mode, text, cli_opts, width, images) -> Rendered` (única função que `App::rerender` e o modo plain usam):

```rust
match mode {
    cli::Mode::Markdown => render::render_with(text, &opts),
    cli::Mode::Json => data::render_json(text, json_path.as_deref(), width),
    cli::Mode::Table { .. } => unreachable!("tratado na Task 9"),
}
```

`App` guarda `mode: cli::Mode`, `json_path: Option<String>`; `raw` (Task 6) continua valendo para qualquer modo. Para a Task 9 o braço `Table` é preenchido; até lá, `Table` retorna `render::error_rendered("CSV/TSV ainda não suportado")` em vez de `unreachable!` para não panicar no meio do desenvolvimento.

- [ ] **Step 6: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS.

- [ ] **Step 7: Verificação manual**

Run: `echo '{"a":{"b":[1,2,{"c":"x"}]},"z":null}' | cargo run --release -- json -p - --path .a.b`
Expected: resumo `lista · 3 itens` e JSON colorido só do trecho. `cargo run --release -- json -p arquivo.json` e `mdv arquivo.json` (por extensão) funcionam igual.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -m "feat(dados): mdv json com --path, suporte a JSONL e saneamento"
```

---

### Task 9: Visualizador CSV/TSV (`mdv csv|tsv`)

**Files:**
- Modify: `Cargo.toml`, `src/data.rs`, `src/render.rs` (`render_table_only`), `src/main.rs` (braço `Table`)

**Interfaces:**
- Consumes: `Cli.{head, cols, no_header}`, `Mode::Table { delimiter }`, `data::MAX_DATA_BYTES`.
- Produces:
  ```rust
  // render.rs
  pub fn render_table_only(rows: Vec<Vec<String>>, header: bool, aligns: Vec<Alignment>, width: usize) -> Rendered
  // data.rs
  pub struct TableOpts { pub delimiter: u8, pub head: usize, pub cols: Vec<String>, pub no_header: bool }
  pub fn render_table_data(text: &str, o: &TableOpts, width: usize) -> Rendered
  ```

- [ ] **Step 1: Dependência**

```toml
csv = "1"
```

- [ ] **Step 2: Testes que falham**

Em `src/data.rs`, `mod tests`:

```rust
fn opts() -> TableOpts {
    TableOpts { delimiter: b',', head: 1000, cols: vec![], no_header: false }
}

#[test]
fn csv_vira_tabela_com_cabecalho_e_rodape() {
    let t = text(&render_table_data("id,nome\n1,Ana\n2,Bia\n", &opts(), 60));
    assert!(t.contains("id") && t.contains("Ana") && t.contains("Bia"), "{t}");
    assert!(t.contains("2 de 2 linhas") && t.contains("2 colunas"), "{t}");
}

#[test]
fn tsv_usa_tab() {
    let o = TableOpts { delimiter: b'\t', ..opts() };
    let t = text(&render_table_data("a\tb\n1\t2\n", &o, 60));
    assert!(t.contains("│ 1") || t.contains("1 │"), "{t}");
}

#[test]
fn head_limita_linhas_e_informa_total() {
    let o = TableOpts { head: 2, ..opts() };
    let t = text(&render_table_data("n\n1\n2\n3\n4\n", &o, 40));
    assert!(t.contains("2 de 4 linhas"), "{t}");
    assert!(!t.contains("│ 4"), "{t}");
}

#[test]
fn cols_seleciona_por_nome_e_valida() {
    let o = TableOpts { cols: vec!["nome".into()], ..opts() };
    let t = text(&render_table_data("id,nome\n1,Ana\n", &o, 40));
    assert!(t.contains("Ana") && !t.contains("id"), "{t}");
    let o = TableOpts { cols: vec!["xx".into()], ..opts() };
    let e = text(&render_table_data("id,nome\n1,Ana\n", &o, 40));
    assert!(e.contains("'xx'") && e.contains("id") && e.contains("nome"), "{e}");
}

#[test]
fn sem_cabecalho_nao_desenha_separador() {
    let o = TableOpts { no_header: true, ..opts() };
    let t = text(&render_table_data("1,2\n3,4\n", &o, 40));
    assert!(!t.contains('├'), "{t}");
    assert!(t.contains('1') && t.contains('3'));
}

#[test]
fn linhas_irregulares_e_aspas_funcionam() {
    let t = text(&render_table_data("a,b\n\"x, y\",1\nso-uma\n", &opts(), 60));
    assert!(t.contains("x, y") && t.contains("so-uma"), "{t}");
}

#[test]
fn coluna_numerica_alinha_a_direita() {
    let r = render_table_data("n\n1\n100\n", &opts(), 40);
    let t = text(&r);
    let linha_1 = t.lines().find(|l| l.contains("│   1")).unwrap_or("");
    assert!(!linha_1.is_empty(), "{t}");
}

#[test]
fn bom_e_removido() {
    let t = text(&render_table_data("\u{feff}id\n1\n", &opts(), 40));
    assert!(t.contains("id") && !t.contains('\u{feff}'), "{t}");
}

#[test]
fn escape_em_celula_nao_vaza() {
    let r = render_table_data("a\n\u{1b}]0;pwn\u{7}\n", &opts(), 40);
    assert!(!crate::render::to_ansi(&r.lines, false).contains('\x1b'));
}

#[test]
fn tabela_larga_e_rapida() {
    let header = (0..500).map(|i| format!("c{i}")).collect::<Vec<_>>().join(",");
    let row = (0..500).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let csv = format!("{header}\n{row}\n");
    let t0 = std::time::Instant::now();
    let _ = render_table_data(&csv, &opts(), 100);
    assert!(t0.elapsed() < std::time::Duration::from_secs(1));
}
```

Run: `cargo test data::`
Expected: erro de compilação.

- [ ] **Step 3: `render_table_only` em `render.rs`**

```rust
pub fn render_table_only(
    rows: Vec<Vec<String>>,
    header: bool,
    aligns: Vec<Alignment>,
    width: usize,
) -> Rendered {
    let head_style = Style::default().fg(H2).add_modifier(Modifier::BOLD);
    let rows: Vec<Vec<Vec<Piece>>> = rows
        .into_iter()
        .enumerate()
        .map(|(ri, row)| {
            row.into_iter()
                .map(|cell| {
                    let st = if header && ri == 0 { head_style } else { Style::default() };
                    vec![(sanitize(&cell).into_owned(), st)]
                })
                .collect()
        })
        .collect();
    let mut r = Renderer::new(width, None);
    r.render_table(TableState { aligns, rows, cur: Vec::new(), header, in_head: false });
    Rendered { lines: r.lines, headings: Vec::new(), outline: Vec::new() }
}
```

- [ ] **Step 4: Implementar `render_table_data` em `data.rs`**

```rust
use pulldown_cmark::Alignment;

use crate::render::render_table_only;

pub struct TableOpts {
    pub delimiter: u8,
    pub head: usize,
    pub cols: Vec<String>,
    pub no_header: bool,
}

fn is_numeric_column(rows: &[Vec<String>], c: usize, skip_header: bool) -> bool {
    let cells: Vec<&str> = rows
        .iter()
        .skip(usize::from(skip_header))
        .filter_map(|r| r.get(c).map(String::as_str))
        .filter(|s| !s.trim().is_empty())
        .collect();
    !cells.is_empty() && cells.iter().all(|s| s.trim().parse::<f64>().is_ok())
}

pub fn render_table_data(text: &str, o: &TableOpts, width: usize) -> Rendered {
    if text.len() > MAX_DATA_BYTES {
        return error_rendered("entrada grande demais para o modo tabela (limite de 5 MiB)");
    }
    let text = text.trim_start_matches('\u{feff}');
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(o.delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    let keep = o.head + usize::from(!o.no_header);
    let (mut rows, mut total): (Vec<Vec<String>>, usize) = (Vec::new(), 0);
    for rec in rdr.records() {
        let rec = match rec {
            Ok(r) => r,
            Err(e) => return error_rendered(&format!("CSV inválido: {e}")),
        };
        total += 1;
        if rows.len() < keep {
            rows.push(rec.iter().map(str::to_string).collect());
        }
    }
    if rows.is_empty() {
        return error_rendered("arquivo vazio");
    }
    let has_header = !o.no_header;
    let data_total = total - usize::from(has_header);

    if !o.cols.is_empty() {
        if !has_header {
            return error_rendered("--cols precisa de cabeçalho (remova --no-header)");
        }
        let header = rows[0].clone();
        let mut idx = Vec::new();
        for name in &o.cols {
            match header.iter().position(|h| h == name) {
                Some(i) => idx.push(i),
                None => {
                    return error_rendered(&format!(
                        "a coluna '{name}' não existe (disponíveis: {})",
                        header.join(", ")
                    ))
                }
            }
        }
        rows = rows
            .into_iter()
            .map(|r| idx.iter().map(|&i| r.get(i).cloned().unwrap_or_default()).collect())
            .collect();
    }

    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let aligns: Vec<Alignment> = (0..ncols)
        .map(|c| if is_numeric_column(&rows, c, has_header) { Alignment::Right } else { Alignment::Left })
        .collect();
    let shown = rows.len() - usize::from(has_header);
    let mut r = render_table_only(rows, has_header, aligns, width);
    r.lines.push(Line::from(Span::styled(
        format!("{shown} de {data_total} linhas · {ncols} colunas"),
        Style::default().fg(crate::render::DIM).add_modifier(Modifier::ITALIC),
    )));
    r
}
```

- [ ] **Step 5: Rodar e ver passar**

Run: `cargo test data::`
Expected: todos PASS. O teste `tabela_larga_e_rapida` depende do item 3 da Fase 1; se falhar por tempo, o item 3 não foi concluído.

- [ ] **Step 6: Ligar no `main.rs`**

Braço de `render_source`:

```rust
cli::Mode::Table { delimiter } => data::render_table_data(
    text,
    &data::TableOpts { delimiter, head: cli.head, cols: cli.cols.clone(), no_header: cli.no_header },
    width,
),
```

`App` guarda `head`, `cols`, `no_header` (ou uma `TableOpts` clonável). Substituir o braço provisório da Task 8.

- [ ] **Step 7: Rodar tudo**

Run: `cargo test && cargo clippy`
Expected: PASS.

- [ ] **Step 8: Verificação manual**

Run: `printf 'id,nome,nota\n1,Ana,9.5\n2,Bia,10\n' | cargo run --release -- csv -p -`
Expected: tabela com `nota` alinhada à direita e rodapé `2 de 2 linhas · 3 colunas`.
Run: `printf 'a\tb\n1\t2\n' | cargo run --release -- tsv -p --cols b -`
Expected: só a coluna `b`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -m "feat(dados): mdv csv/tsv com --head, --cols, --no-header e alinhamento numérico"
```

---

### Task 10: Documentação e amostras

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `src/main.rs` (`HELP`)
- Create: `samples/llm.md`, `samples/mermaid.md`, `samples/dados.json`, `samples/dados.csv`, `samples/dados.tsv`

- [ ] **Step 1: `HELP`** — adicionar `mdv json|csv|tsv [opções] <arquivo|->`, as opções `--remote`, `--mermaid`, `--follow`, `--latest`, `--toc`, `--path`, `--head`, `--cols`, `--no-header`, `--delimiter` e a tecla `m`.
- [ ] **Step 2: `README.md`** — seções "Imagens" (meio-bloco, `--remote`, limites e política de rede privada), "Mermaid" (`--mermaid`, `mmdc`, `MDV_MMDC`), "Markdown de agentes/LLMs", "JSON, CSV e TSV"; tirar "Imagens aparecem como `[imagem: …]`" de Limitações e atualizar "Próximos passos" (protocolo Kitty/Sixel passa a ser um refinamento opcional sobre o meio-bloco).
- [ ] **Step 3: `CLAUDE.md`** — atualizar "Arquitetura" (módulos novos, `ImageStore`, regra "I/O fica fora de `render.rs`") e a linha "Não há testes automatizados" (agora há; citar `cargo test`).
- [ ] **Step 4: Amostras** — um arquivo por recurso, pequenos, usados nas verificações manuais acima.
- [ ] **Step 5: Verificação final**

Run: `cargo test && cargo clippy -- -W clippy::pedantic 2>&1 | tail -5 && for f in samples/llm.md samples/mermaid.md samples/dados.json samples/dados.csv samples/dados.tsv; do cargo run --release -q -- -p $f >/dev/null || echo FALHOU $f; done`
Expected: testes PASS; nenhum `FALHOU`.

- [ ] **Step 6: Commit**

```bash
git add README.md CLAUDE.md src samples
git commit -m "docs: imagens, mermaid, markdown de LLMs e modos JSON/CSV/TSV"
```

---

## Self-Review

**Cobertura do pedido:** 1) Mermaid → Task 4. 2) Imagens por URL → Task 3. 3) Imagens → Task 2. 4) MD de agentes/LLMs → Tasks 5–7 (interpretação declarada em Global Constraints). 5) JSON/CSV/TSV com comandos próprios → Tasks 1, 8, 9.

**Placeholders:** nenhum "TBD/TODO"; a única ponte provisória (braço `Table` retornando erro na Task 8) é explícita e substituída na Task 9.

**Consistência de tipos:** `ImageStore::{new, with_remote, with_mermaid, get, get_mermaid, get_job, version, mermaid_enabled}`, `RenderOpts { width, images }`, `render_with`, `render_code_only`, `render_table_only`, `error_rendered`, `toc`, `OutlineEntry`, `Cli`/`Mode`/`Parsed`, `TableOpts` usados com as mesmas assinaturas em todas as tasks. `sanitize` e `Renderer::new` são citados como existentes/extraídos (Fase 1 item 1; Task 6 passo 2).

**Riscos conhecidos:** nomes exatos de API de `ureq 2`/`image 0.25`/`pulldown-cmark 0.13` foram escritos de memória — se o compilador divergir, ajustar a chamada mantendo a interface e o teste. Imagens ficam em meio-bloco (resolução baixa); protocolo Kitty/Sixel é refinamento futuro. SVG e `data:` URIs não são suportados.
