//! Decodificação de imagens e conversão para linhas de meio-bloco.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, Limits};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::net::{self, FetchPolicy};

pub const MAX_REMOTE_IMAGES: usize = 32;
pub const MAX_LOCAL_IMAGES: usize = 64;
pub const MAX_MERMAID_DIAGRAMS: usize = 16;
const MERMAID_PREFIX: &str = "mermaid:";
const LOCAL_PREFIX: &str = "file:";
const DATA_PREFIX: &str = "data:";
pub const MAX_DATA_IMAGES: usize = 32;
/// Maior carga base64 aceita num `data:` (equivale a `MAX_IMAGE_BYTES` já decodificados).
#[allow(clippy::cast_possible_truncation)] // 10 MiB cabe com folga em usize (mesmo em 32-bit)
pub const MAX_DATA_PAYLOAD: usize = (MAX_IMAGE_BYTES as usize) / 3 * 4 + 8;
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// Tetos da imagem no texto. 120 colunas cabem nos 128 diacríticos do Kitty (`graphics.rs`).
pub const MAX_IMAGE_COLS: usize = 120;
pub const MAX_IMAGE_ROWS: usize = 36;
const MAX_DIMENSION: u32 = 8192;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;
/// Com gráficos ligados, o cache guarda imagens de até este tamanho (px): dá nitidez no
/// terminal sem estourar a memória (≈1,8 MiB por imagem, no máximo ~112 imagens por documento).
const GFX_MAX_W: u32 = 800;
const GFX_MAX_H: u32 = 600;

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
    limits.max_alloc = Some(crate::mem::image_budget(crate::mem::used(), crate::mem::limit(), MAX_ALLOC));
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

/// Cada linha de texto mostra duas linhas de pixels: `▀` com cor de frente = pixel de cima
/// e fundo = pixel de baixo. Pixels com alfa < 128 ficam transparentes.
pub fn halfblock(img: &DynamicImage, cols: usize, max_rows: usize) -> Vec<Line<'static>> {
    let cols = u32::try_from(cols.max(1)).unwrap_or(u32::MAX);
    let max_h = u32::try_from(max_rows.max(1) * 2).unwrap_or(u32::MAX);
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
    // Só abre arquivo regular: abrir uma FIFO ou um dispositivo pode bloquear para sempre.
    if !std::fs::metadata(&path).map_err(|e| e.to_string())?.is_file() {
        return Err("não é um arquivo regular".into());
    }
    // Confere de novo no arquivo aberto (o caminho pode ter sido trocado entre as duas checagens)
    // e usa o tamanho e a leitura desse mesmo arquivo.
    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("não é um arquivo regular".into());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err("imagem grande demais".into());
    }
    decode(&read_capped(file, MAX_IMAGE_BYTES)?)
}

/// Lê até `max` bytes; passar disso é erro. O tamanho dos metadados não basta: arquivos de
/// procfs/sysfs dizem ter 0 bytes e não terminam.
fn read_capped(reader: impl Read, max: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    reader.take(max + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err("imagem grande demais".into());
    }
    Ok(buf)
}

/// Guarda no cache só o que cabe em `cap` (px): uma imagem de 8192×8192 ocuparia 256 MiB e
/// seria reduzida de novo a cada redesenho.
fn to_state(r: Result<DynamicImage, String>, cap: (u32, u32)) -> ImageState {
    match r {
        Ok(img) => {
            let (max_w, max_h) = cap;
            let img = if img.width() > max_w || img.height() > max_h {
                img.resize(max_w, max_h, FilterType::Triangle)
            } else {
                img
            };
            ImageState::Ready(Arc::new(img))
        }
        Err(e) => ImageState::Failed(e),
    }
}

pub fn is_remote(src: &str) -> bool {
    let s = src.to_ascii_lowercase();
    s.starts_with("http://") || s.starts_with("https://")
}

/// `src` começa com `data:` (qualquer caixa)?
pub fn is_data(src: &str) -> bool {
    src.len() >= DATA_PREFIX.len() && src.as_bytes()[..DATA_PREFIX.len()].eq_ignore_ascii_case(DATA_PREFIX.as_bytes())
}

/// Decodifica `data:image/<tipo>;base64,<carga>`. Só base64; SVG não é suportado.
fn decode_data_uri(src: &str) -> Result<DynamicImage, String> {
    // `is_data` garante que os 5 primeiros bytes são ASCII, então o corte é seguro.
    let rest = &src[DATA_PREFIX.len()..];
    let (meta, payload) = rest.split_once(',').ok_or("data: sem vírgula")?;
    let mut parts = meta.split(';');
    let mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let is_base64 = parts.any(|p| p.trim().eq_ignore_ascii_case("base64"));
    if mime == "image/svg+xml" {
        return Err("SVG não é suportado".into());
    }
    if !mime.starts_with("image/") {
        return Err("data: não é uma imagem".into());
    }
    if !is_base64 {
        return Err("data: sem base64 não é suportado".into());
    }
    if payload.len() > MAX_DATA_PAYLOAD {
        return Err("imagem grande demais".into());
    }
    let bytes = crate::clip::base64_decode(payload).ok_or("base64 inválido")?;
    decode(&bytes)
}


/// Teto de entradas no cache gráfico; além dele o chamador cai para meio-bloco.
pub const MAX_GRAPHICS: usize = 256;

/// Imagem pronta para o protocolo gráfico: `id` do terminal e os bytes PNG.
#[derive(Clone)]
pub struct Graphic {
    pub id: u32,
    pub png: Arc<Vec<u8>>,
}

pub struct ImageStore {
    base_dir: Option<PathBuf>,
    cache: Arc<Mutex<HashMap<String, ImageState>>>,
    version: Arc<AtomicU64>,
    remote: Option<(Arc<FetchPolicy>, bool)>, // (política, bloqueante)
    mermaid: Option<bool>, // Some(bloqueante) quando habilitado
    mermaid_hold: AtomicBool,
    /// PNG original de cada diagrama (chave de `mermaid_key`): o cache de imagens guarda só a
    /// versão reduzida, e o zoom (`imgzoom`) decodifica este sob demanda, um por vez.
    mermaid_png: Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>,
    graphics: bool,
    /// Teto de pixels guardado no cache; padrão: o do terminal.
    max_pixels: Option<(u32, u32)>,
    gfx: Mutex<HashMap<(usize, usize, usize), Graphic>>, // (ponteiro da imagem, colunas, linhas)
    /// Próximo `id` gráfico; só cresce, porque o terminal guarda as imagens já enviadas.
    next_gfx: AtomicU32,
    /// Chaves pedidas desde o último `begin_pass`; os limites por documento contam só elas.
    used: Mutex<HashSet<String>>,
}

impl ImageStore {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        ImageStore {
            base_dir,
            cache: Arc::new(Mutex::new(HashMap::new())),
            version: Arc::new(AtomicU64::new(0)),
            remote: None,
            mermaid: None,
            mermaid_hold: AtomicBool::new(false),
            mermaid_png: Arc::new(Mutex::new(HashMap::new())),
            graphics: false,
            max_pixels: None,
            gfx: Mutex::new(HashMap::new()),
            next_gfx: AtomicU32::new(1),
            used: Mutex::new(HashSet::new()),
        }
    }

    pub fn with_graphics(mut self, on: bool) -> Self {
        self.graphics = on;
        self
    }

    /// Guarda as imagens em até `w`×`h` pixels (a exportação precisa de mais que o terminal).
    pub fn with_max_pixels(mut self, w: u32, h: u32) -> Self {
        self.max_pixels = Some((w, h));
        self
    }

    pub fn graphics_enabled(&self) -> bool {
        self.graphics
    }

    /// Teto de pixels guardado no cache.
    fn cap(&self) -> (u32, u32) {
        if let Some(px) = self.max_pixels {
            px
        } else if self.graphics {
            (GFX_MAX_W, GFX_MAX_H)
        } else {
            (
                u32::try_from(MAX_IMAGE_COLS).unwrap_or(u32::MAX),
                u32::try_from(MAX_IMAGE_ROWS * 2).unwrap_or(u32::MAX),
            )
        }
    }

    /// `id` e PNG de `img` desenhada em `cols`×`rows` células. `None` com gráficos desligados.
    /// A chave é o endereço da imagem do cache mais o tamanho em células; `begin_pass` tira daqui
    /// as imagens que despeja, para o endereço não ser confundido se a memória for reaproveitada.
    pub fn graphic(&self, img: &Arc<DynamicImage>, cols: usize, rows: usize) -> Option<Graphic> {
        if !self.graphics {
            return None;
        }
        let key = (Arc::as_ptr(img) as usize, cols, rows);
        let mut map = self.gfx.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(g) = map.get(&key) {
            return Some(g.clone());
        }
        if map.len() >= MAX_GRAPHICS {
            return None;
        }
        let png = Arc::new(crate::graphics::encode_png(img)?);
        let id = Some(self.next_gfx.fetch_add(1, Ordering::Relaxed)).filter(|&i| i <= 0x00FF_FFFF)?;
        let g = Graphic { id, png };
        map.insert(key, g.clone());
        Some(g)
    }

    /// Diagrama só vira figura com o protocolo gráfico ativo; sem ele o bloco fica como código.
    pub fn mermaid_enabled(&self) -> bool {
        crate::mermaid_native::available() && self.mermaid.is_some() && self.graphics
    }

    /// Mermaid só com o renderizador nativo (padrão): em processo, sem subprocesso.
    pub fn with_native_mermaid(mut self, blocking: bool) -> Self {
        self.mermaid = Some(blocking);
        self
    }

    /// Enquanto `hold`, `get_mermaid` não dispara o job (usado com `--follow`).
    pub fn set_mermaid_hold(&self, hold: bool) {
        self.mermaid_hold.store(hold, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn mermaid_held(&self) -> bool {
        self.mermaid_hold.load(Ordering::Relaxed)
    }

    /// Chave do diagrama no cache (e em `mermaid_png`).
    pub fn mermaid_key(source: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        format!("{MERMAID_PREFIX}{:016x}", h.finish())
    }

    /// PNG original do diagrama `key`, se já foi renderizado.
    pub fn mermaid_png(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.mermaid_png.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
    }

    pub fn get_mermaid(&self, source: &str) -> ImageState {
        let Some(blocking) = self.mermaid else {
            return ImageState::Failed("mermaid desativado".into());
        };
        let key = Self::mermaid_key(source);
        if self.mermaid_hold.load(Ordering::Relaxed) {
            // Entrada ainda chegando: o bloco pode estar incompleto, então nada é disparado.
            // A chave conta como usada para o diagrama já pronto não ser despejado.
            self.used().insert(key);
            return ImageState::Loading;
        }
        if self.over_limit(&key, |k| k.starts_with(MERMAID_PREFIX), MAX_MERMAID_DIAGRAMS) {
            return ImageState::Failed("limite de diagramas por documento atingido".into());
        }
        let src = source.to_string();
        let (pngs, png_key) = (Arc::clone(&self.mermaid_png), key.clone());
        self.get_job(&key, !blocking, move || {
            let png = crate::mermaid_native::render_png(&src)?;
            let img = decode(&png)?;
            pngs.lock().unwrap_or_else(PoisonError::into_inner).insert(png_key, Arc::new(png));
            Ok(img)
        })
    }

    pub fn with_remote(mut self, policy: FetchPolicy, blocking: bool) -> Self {
        self.remote = Some((Arc::new(policy), blocking));
        self
    }

    /// Incrementa a cada download em segundo plano concluído.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, ImageState>> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn used(&self) -> MutexGuard<'_, HashSet<String>> {
        self.used.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `key` seria mais uma imagem do seu tipo além de `max` nesta passada?
    fn over_limit(&self, key: &str, same_kind: impl Fn(&str) -> bool, max: usize) -> bool {
        let used = self.used();
        !used.contains(key) && used.iter().filter(|k| same_kind(k)).count() >= max
    }

    /// Começa uma nova renderização: despeja o que a passada anterior não pediu (versões
    /// antigas de diagramas e imagens que saíram do documento) e zera a contagem dos limites.
    /// O cache fica limitado a duas passadas; o que segue em uso volta do cache sem recalcular.
    pub fn begin_pass(&self) {
        let used = std::mem::take(&mut *self.used());
        let mut evicted = HashSet::new();
        self.lock().retain(|k, st| {
            let keep = used.contains(k);
            if let (false, ImageState::Ready(img)) = (keep, &st) {
                evicted.insert(Arc::as_ptr(img) as usize);
            }
            keep
        });
        self.mermaid_png.lock().unwrap_or_else(PoisonError::into_inner).retain(|k, _| used.contains(k));
        if !evicted.is_empty() {
            self.gfx.lock().unwrap_or_else(PoisonError::into_inner).retain(|(p, _, _), _| !evicted.contains(p));
        }
    }

    /// Executa `job` (agora ou em thread) e guarda o resultado sob `key`.
    pub fn get_job(
        &self,
        key: &str,
        run_async: bool,
        job: impl FnOnce() -> Result<DynamicImage, String> + Send + 'static,
    ) -> ImageState {
        self.used().insert(key.to_string());
        if let Some(s) = self.lock().get(key) {
            return s.clone();
        }
        let cap = self.cap();
        if !run_async {
            let st = to_state(job(), cap);
            self.lock().insert(key.to_string(), st.clone());
            return st;
        }
        self.lock().insert(key.to_string(), ImageState::Loading);
        let (cache, version, key) = (Arc::clone(&self.cache), Arc::clone(&self.version), key.to_string());
        std::thread::spawn(move || {
            let st = to_state(job(), cap);
            cache.lock().unwrap_or_else(PoisonError::into_inner).insert(key, st);
            version.fetch_add(1, Ordering::Relaxed);
        });
        ImageState::Loading
    }

    /// O destino de uma imagem em Markdown vem codificado (`a%20b.png`): usa o nome literal se
    /// existir, senão o decodificado.
    fn local_name(&self, src: &str) -> String {
        let exists = |n: &str| match &self.base_dir {
            Some(b) => b.join(n).exists(),
            None => Path::new(n).exists(),
        };
        if src.contains('%') && !exists(src) {
            let decoded = mdv_text::percent_decode(src);
            if exists(&decoded) {
                return decoded;
            }
        }
        src.to_string()
    }

    pub fn get(&self, src: &str) -> ImageState {
        if is_data(src) {
            // Recusa o gigante antes de gastar hash, cache ou cópia (o cabeçalho cabe em 1 KiB).
            let comma = src.as_bytes().iter().take(1024).position(|&b| b == b',');
            if comma.map_or(src.len() > 1024, |c| src.len() - c - 1 > MAX_DATA_PAYLOAD) {
                return ImageState::Failed("imagem grande demais".into());
            }
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            src.hash(&mut h);
            let key = format!("{DATA_PREFIX}{:016x}", h.finish());
            if self.over_limit(&key, |k| k.starts_with(DATA_PREFIX), MAX_DATA_IMAGES) {
                return ImageState::Failed("limite de imagens data: atingido".into());
            }
            // Acerto de cache não copia o URI.
            if let Some(st) = self.lock().get(&key).cloned() {
                self.used().insert(key);
                return st;
            }
            let s = src.to_string();
            return self.get_job(&key, false, move || decode_data_uri(&s));
        }
        if !is_remote(src) {
            let src = &self.local_name(src);
            // A chave é o caminho canônico: `a.png`, `./a.png` e `x/../a.png` são um só arquivo.
            let path = match &self.base_dir {
                Some(b) => b.join(src),
                None => PathBuf::from(src),
            };
            let key = format!(
                "{LOCAL_PREFIX}{}",
                std::fs::canonicalize(&path).unwrap_or(path).to_string_lossy()
            );
            if self.over_limit(&key, |k| k.starts_with(LOCAL_PREFIX), MAX_LOCAL_IMAGES) {
                return ImageState::Failed("limite de imagens locais atingido".into());
            }
            let base = self.base_dir.clone();
            let s = src.to_string();
            return self.get_job(&key, false, move || load_local(base.as_deref(), &s));
        }
        let Some((policy, blocking)) = &self.remote else {
            return ImageState::Failed("imagem remota: use --remote".into());
        };
        if self.over_limit(src, is_remote, MAX_REMOTE_IMAGES) {
            return ImageState::Failed("limite de imagens remotas atingido".into());
        }
        let (policy, url) = (Arc::clone(policy), src.to_string());
        self.get_job(src, !*blocking, move || decode(&net::fetch(&url, &policy)?))
    }
}

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

    fn grava_png(dir: &std::path::Path, nome: &str, w: u32, h: u32) {
        std::fs::write(dir.join(nome), png_bytes(w, h)).unwrap();
    }

    #[test]
    fn caminho_local_com_percent_encoding_acha_o_arquivo() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "meu diagrama.png", 8, 8);
        grava_png(dir.path(), "lit%20eral.png", 8, 8);
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        assert!(matches!(store.get("meu%20diagrama.png"), ImageState::Ready(_)));
        assert!(matches!(store.get("lit%20eral.png"), ImageState::Ready(_)), "nome literal tem prioridade");
    }

    #[test]
    fn max_pixels_troca_o_teto_do_cache() {
        let d = tempfile::tempdir().unwrap();
        grava_png(d.path(), "g.png", 300, 200);
        let pequeno = ImageStore::new(Some(d.path().to_path_buf()));
        let ImageState::Ready(a) = pequeno.get("g.png") else { panic!() };
        assert!(a.width() as usize <= MAX_IMAGE_COLS);
        let grande = ImageStore::new(Some(d.path().to_path_buf())).with_max_pixels(1200, 1200);
        let ImageState::Ready(b) = grande.get("g.png") else { panic!() };
        assert_eq!((b.width(), b.height()), (300, 200));
    }

    #[test]
    fn com_graficos_a_imagem_guarda_mais_pixels() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "a.png", 1000, 800);
        let normal = ImageStore::new(Some(dir.path().to_path_buf()));
        let gfx = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let (ImageState::Ready(n), ImageState::Ready(g)) = (normal.get("a.png"), gfx.get("a.png")) else {
            panic!("esperava Ready");
        };
        assert!(n.width() as usize <= MAX_IMAGE_COLS && n.height() as usize <= MAX_IMAGE_ROWS * 2);
        assert!(g.width() <= GFX_MAX_W && g.height() <= GFX_MAX_H && g.width() as usize > MAX_IMAGE_COLS);
    }

    #[test]
    fn graphic_reaproveita_o_id_por_imagem_e_tamanho() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "a.png", 8, 8);
        let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let ImageState::Ready(img) = store.get("a.png") else { panic!() };
        let a = store.graphic(&img, 10, 5).unwrap();
        let b = store.graphic(&img, 10, 5).unwrap();
        let c = store.graphic(&img, 12, 5).unwrap();
        assert_eq!(a.id, b.id);
        assert_ne!(a.id, c.id);
        assert_eq!(&a.png[..4], b"\x89PNG");
    }

    #[test]
    fn graphic_respeita_o_teto_do_cache() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "a.png", 4, 4);
        let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let ImageState::Ready(img) = store.get("a.png") else { panic!() };
        for i in 0..MAX_GRAPHICS {
            let g = store.graphic(&img, i + 1, 1).expect("abaixo do teto");
            assert!((1..=0x00FF_FFFF).contains(&g.id));
        }
        assert!(store.graphic(&img, MAX_GRAPHICS + 1, 1).is_none());
        assert!(store.graphic(&img, 1, 1).is_some(), "chave existente continua");
    }

    #[test]
    fn graphic_e_none_com_graficos_desligados() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "a.png", 8, 8);
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let ImageState::Ready(img) = store.get("a.png") else { panic!() };
        assert!(store.graphic(&img, 4, 2).is_none());
        assert!(!store.graphics_enabled());
    }

    pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(w, h, image::Rgba([9, 9, 9, 255])));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn data_uri(png: &[u8]) -> String {
        format!("data:image/png;base64,{}", crate::clip::base64(png))
    }

    #[test]
    fn data_uri_png_vira_imagem() {
        let store = ImageStore::new(None);
        match store.get(&data_uri(&png_bytes(4, 4))) {
            ImageState::Ready(img) => assert_eq!((img.width(), img.height()), (4, 4)),
            _ => panic!("esperava Ready"),
        }
    }

    #[test]
    fn data_uri_aceita_maiusculas_no_esquema_e_no_tipo() {
        let uri = format!("DATA:IMAGE/PNG;BASE64,{}", crate::clip::base64(&png_bytes(2, 2)));
        assert!(matches!(ImageStore::new(None).get(&uri), ImageState::Ready(_)));
    }

    fn falha(uri: &str) -> String {
        match ImageStore::new(None).get(uri) {
            ImageState::Failed(e) => e,
            _ => panic!("esperava Failed para {uri}"),
        }
    }

    #[test]
    fn data_uri_hostil_vira_falha_com_motivo() {
        assert!(falha("data:image/svg+xml;base64,PHN2Zy8+").contains("SVG"));
        assert!(falha("data:image/png,abc").contains("base64"));
        assert!(falha("data:text/plain;base64,QUJD").contains("não é uma imagem"));
        assert!(falha("data:image/png;base64").contains("vírgula"));
        assert!(falha("data:image/png;base64,@@@@").contains("base64"));
        // base64 válido ("ABC"), mas os bytes não são uma imagem: falha do decodificador, sem pânico
        assert!(!falha("data:image/png;base64,QUJD").is_empty());
    }

    #[test]
    fn data_uri_acima_do_teto_e_recusada_antes_de_decodificar() {
        let uri = format!("data:image/png;base64,{}", "A".repeat(MAX_DATA_PAYLOAD + 1));
        assert!(falha(&uri).contains("grande"));
    }

    #[test]
    fn data_uri_gigante_nao_ocupa_vaga_nem_e_processada() {
        let store = ImageStore::new(None);
        for i in 0..=MAX_DATA_IMAGES {
            let uri = format!("data:image/png;base64,{}{i}", "A".repeat(MAX_DATA_PAYLOAD + 1));
            assert!(matches!(store.get(&uri), ImageState::Failed(m) if m.contains("grande")));
        }
        let ok = format!("data:image/png;base64,{}", crate::clip::base64(&png_bytes(4, 4)));
        assert!(matches!(store.get(&ok), ImageState::Ready(_)));
    }

    #[test]
    fn data_uri_tem_limite_por_documento() {
        let store = ImageStore::new(None);
        for i in 0..MAX_DATA_IMAGES {
            let w = u32::try_from(i).unwrap() + 1;
            assert!(matches!(store.get(&data_uri(&png_bytes(w, 1))), ImageState::Ready(_)));
        }
        let extra = data_uri(&png_bytes(500, 1));
        assert!(matches!(store.get(&extra), ImageState::Failed(e) if e.contains("limite")));
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
    fn leitura_limitada_recusa_fonte_infinita() {
        // arquivos de procfs/sysfs informam tamanho 0 mas não têm fim
        let e = read_capped(std::io::repeat(0), 1024).unwrap_err();
        assert!(e.contains("grande"), "{e}");
        assert_eq!(read_capped(&b"abc"[..], 1024).unwrap(), b"abc");
    }

    #[cfg(unix)]
    #[test]
    fn fifo_nao_trava_a_leitura() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe.png");
        // abrir uma FIFO para leitura bloqueia até aparecer um escritor
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        let (tx, rx) = std::sync::mpsc::channel();
        let base = dir.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(matches!(ImageStore::new(Some(base)).get("pipe.png"), ImageState::Failed(_)));
        });
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(3)), Ok(true), "get() travou na FIFO");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn arquivo_de_procfs_sem_tamanho_nao_estoura_memoria() {
        let store = ImageStore::new(None);
        // /proc/self/maps: metadata().len() == 0, conteúdo não é imagem
        assert!(matches!(store.get("/proc/self/maps"), ImageState::Failed(_)));
    }

    #[test]
    fn cache_guarda_a_imagem_ja_reduzida() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("grande.png"), png_bytes(400, 300)).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let ImageState::Ready(img) = store.get("grande.png") else { panic!("deveria carregar") };
        assert!(img.width() as usize <= MAX_IMAGE_COLS && img.height() as usize <= MAX_IMAGE_ROWS * 2);
    }

    #[test]
    fn grafias_diferentes_do_mesmo_arquivo_compartilham_o_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.png"), png_bytes(4, 4)).unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        let (ImageState::Ready(x), ImageState::Ready(y)) = (store.get("a.png"), store.get("./a.png")) else {
            panic!("deveria carregar")
        };
        assert!(Arc::ptr_eq(&x, &y));
    }

    #[test]
    fn limite_de_imagens_locais_distintas() {
        let dir = tempfile::tempdir().unwrap();
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        for i in 0..MAX_LOCAL_IMAGES {
            store.get(&format!("{i}.png"));
        }
        match store.get("extra.png") {
            ImageState::Failed(m) => assert!(m.contains("limite"), "{m}"),
            _ => panic!("deveria estourar o limite"),
        }
    }

    #[test]
    fn limite_de_diagramas_mermaid_distintos() {
        let store = ImageStore::new(None).with_native_mermaid(true);
        for i in 0..MAX_MERMAID_DIAGRAMS {
            store.get_mermaid(&format!("isto não é mermaid {i}"));
        }
        match store.get_mermaid("graph TD; extra-->B") {
            ImageState::Failed(m) => assert!(m.contains("limite"), "{m}"),
            _ => panic!("deveria estourar o limite"),
        }
    }

    /// Live reload: cada edição do diagrama vira uma chave nova; as versões antigas não
    /// podem contar contra o limite nem ficar no cache para sempre.
    #[test]
    fn versoes_antigas_do_diagrama_nao_esgotam_o_limite() {
        let store = ImageStore::new(None).with_native_mermaid(true);
        for i in 0..=MAX_MERMAID_DIAGRAMS * 2 {
            store.begin_pass();
            if let ImageState::Failed(m) = store.get_mermaid(&format!("isto não é mermaid {i}")) {
                assert!(!m.contains("limite"), "versão {i}: {m}");
            }
        }
        assert!(store.lock().len() <= 2, "só a passada anterior e a atual ficam no cache");
    }

    #[test]
    fn passada_descarta_o_que_a_anterior_nao_usou() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "a.png", 2, 2);
        grava_png(dir.path(), "b.png", 2, 2);
        let store = ImageStore::new(Some(dir.path().to_path_buf()));
        store.get("a.png");
        store.begin_pass();
        store.get("a.png");
        store.get("b.png");
        store.begin_pass();
        store.get("b.png");
        store.begin_pass();
        let keys: Vec<String> = store.lock().keys().cloned().collect();
        assert_eq!(keys.len(), 1, "{keys:?}");
        assert!(keys[0].ends_with("b.png"), "{keys:?}");
    }

    #[test]
    fn graphic_nao_reusa_id_depois_do_despejo() {
        let dir = tempfile::tempdir().unwrap();
        grava_png(dir.path(), "a.png", 2, 2);
        grava_png(dir.path(), "b.png", 2, 2);
        let store = ImageStore::new(Some(dir.path().to_path_buf())).with_graphics(true);
        let ImageState::Ready(a) = store.get("a.png") else { panic!() };
        let ga = store.graphic(&a, 2, 1).unwrap();
        drop(a);
        store.begin_pass();
        store.begin_pass(); // a.png sai do cache
        let ImageState::Ready(b) = store.get("b.png") else { panic!() };
        let gb = store.graphic(&b, 2, 1).unwrap();
        assert_ne!(ga.id, gb.id, "o terminal ainda guarda a imagem antiga sob o id velho");
    }

    #[test]
    fn mermaid_invalido_falha_sem_sugerir_flags_removidas() {
        let store = ImageStore::new(None).with_native_mermaid(true);
        match store.get_mermaid("isto não é mermaid") {
            ImageState::Failed(m) => assert!(!m.contains("--mermaid") && !m.contains("setup"), "{m}"),
            _ => panic!("deveria falhar"),
        }
    }

    #[test]
    fn mermaid_nativo_desenha() {
        if !crate::mermaid_native::fonts_available() {
            return;
        }
        let store = ImageStore::new(None).with_native_mermaid(true);
        assert!(matches!(store.get_mermaid("graph TD\n A-->B\n"), ImageState::Ready(_)));
    }

    #[test]
    fn mermaid_em_espera_nao_dispara_o_job() {
        let store = ImageStore::new(None).with_native_mermaid(true);
        store.set_mermaid_hold(true);
        assert!(matches!(store.get_mermaid("isto não é mermaid"), ImageState::Loading));
        // nada foi registrado: ao liberar, o job roda (e falha, pois a fonte é inválida)
        store.set_mermaid_hold(false);
        assert!(matches!(store.get_mermaid("isto não é mermaid"), ImageState::Failed(_)));
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

    #[cfg(feature = "remote")]
    use crate::net::FetchPolicy;
    #[cfg(feature = "remote")]
    use std::io::{Read, Write};
    #[cfg(feature = "remote")]
    use std::net::TcpListener;

    #[cfg(feature = "remote")]
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

    #[cfg(feature = "remote")]
    fn local_ok() -> FetchPolicy {
        FetchPolicy { allow_private: true, ..FetchPolicy::default() }
    }

    #[cfg(feature = "remote")]
    #[test]
    fn remoto_bloqueante_devolve_pronto() {
        let store = ImageStore::new(None).with_remote(local_ok(), true);
        assert!(matches!(store.get(&serve_png()), ImageState::Ready(_)));
    }

    #[cfg(feature = "remote")]
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

    #[cfg(feature = "remote")]
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

    #[test]
    fn store_nao_baixa_sem_remote() {
        let store = ImageStore::new(None);
        match store.get("https://example.com/a.png") {
            ImageState::Failed(m) => assert!(m.contains("--remote")),
            _ => panic!("deveria falhar"),
        }
    }

    #[test]
    fn mermaid_so_fica_habilitado_com_graficos() {
        let sem = ImageStore::new(None).with_native_mermaid(true);
        assert!(!sem.mermaid_enabled(), "sem gráficos o diagrama fica como código");
        let com = ImageStore::new(None).with_native_mermaid(true).with_graphics(true);
        assert_eq!(com.mermaid_enabled(), cfg!(feature = "mermaid"));
        assert!(!ImageStore::new(None).with_graphics(true).mermaid_enabled(), "gráficos sem mermaid ligado");
    }

    #[cfg(not(feature = "mermaid"))]
    #[test]
    fn sem_a_feature_mermaid_o_diagrama_nunca_e_habilitado() {
        let store = ImageStore::new(None).with_native_mermaid(true).with_graphics(true);
        assert!(!store.mermaid_enabled());
        let err = crate::mermaid_native::render_png("graph TD; A-->B").unwrap_err();
        assert!(err.contains("recompile com --features mermaid"), "{err}");
    }
}
