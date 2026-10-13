//! Decodificação de imagens e conversão para linhas de meio-bloco.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, Limits};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::net::{self, FetchPolicy};

pub const MAX_REMOTE_IMAGES: usize = 32;
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

    /// Incrementa a cada download em segundo plano concluído.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, ImageState>> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
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
            cache.lock().unwrap_or_else(PoisonError::into_inner).insert(key, st);
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
        let over_limit = {
            let cache = self.lock();
            !cache.contains_key(src) && cache.keys().filter(|k| is_remote(k)).count() >= MAX_REMOTE_IMAGES
        };
        if over_limit {
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

    #[test]
    fn store_nao_baixa_sem_remote() {
        let store = ImageStore::new(None);
        match store.get("https://example.com/a.png") {
            ImageState::Failed(m) => assert!(m.contains("--remote")),
            _ => panic!("deveria falhar"),
        }
    }
}
