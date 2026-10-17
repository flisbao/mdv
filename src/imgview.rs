//! Zoom de imagem na TUI (`i`): um diagrama Mermaid na tela inteira, com zoom e deslocamento.
//!
//! O cache de imagens guarda só a versão reduzida do diagrama; aqui o PNG original é
//! decodificado sob demanda (um por vez) e recortado a cada movimento.

use std::sync::Arc;

use image::imageops::FilterType;
use image::DynamicImage;
use crate::app::Modal;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::graphics;
use crate::image::halfblock;
use crate::imgzoom::{self, Frame as Crop, View};
use crate::App;

/// `id` do terminal usado pelo zoom (fora da faixa que `ImageStore::graphic` distribui).
const ZOOM_ID: u32 = 0x00FF_FF01;
/// Apaga só a imagem do zoom antes de enviar a próxima.
const DELETE_ZOOM: &str = "\x1b_Ga=d,d=I,i=16776961,q=2\x1b\\";
/// Maior lado (px) do recorte enviado ao terminal.
const MAX_CROP_PX: u32 = 1600;
/// Fração da janela que cada `h`/`j`/`k`/`l` desloca.
const PAN_STEP: f64 = 0.25;
/// Fração da janela que cada página desloca.
const PAGE_STEP: f64 = 0.9;

pub struct ImgZoom {
    /// Índice em `Rendered.mermaids`.
    pub index: usize,
    /// Chave do diagrama; se o documento mudar e a chave não bater, o zoom fecha.
    pub key: String,
    img: Arc<DynamicImage>,
    view: View,
    fit: f64,
    max: f64,
    crop: Option<Crop>,
    /// Último desenho: recorte e área usados.
    shown: Option<(Crop, usize, usize)>,
    lines: Vec<Line<'static>>,
}

impl ImgZoom {
    pub fn new(index: usize, key: String, img: DynamicImage) -> Self {
        let view = View::new(img.width(), img.height());
        ImgZoom { index, key, img: Arc::new(img), view, fit: 1.0, max: 1.0, crop: None, shown: None, lines: Vec::new() }
    }

    /// Fator em relação à imagem inteira na tela (1,0 = ajustada).
    pub fn factor(&self) -> f64 {
        self.view.scale(self.fit, self.max) / self.fit
    }

    /// Recalcula o recorte para a área `cols`×`rows` e refaz o desenho se mudou. Com `kitty`,
    /// devolve a sequência que envia o recorte ao terminal; sem ele, desenha em meio-bloco.
    /// `px_per_col` é a largura de uma célula em pixels (só vale no Kitty).
    pub fn refresh(&mut self, cols: usize, rows: usize, px_per_col: f64, kitty: bool) -> Option<String> {
        let (cols, rows) = if kitty { (cols.min(graphics::MAX_CELLS), rows.min(graphics::MAX_CELLS)) } else { (cols, rows) };
        let (w, h) = (self.img.width(), self.img.height());
        self.fit = imgzoom::fit_scale(w, h, cols, rows);
        self.max = imgzoom::max_scale(self.fit, if kitty { px_per_col } else { 1.0 });
        let scale = self.view.scale(self.fit, self.max);
        let crop = imgzoom::frame(w, h, cols, rows, scale, self.view.cx, self.view.cy);
        self.view.settle(&crop);
        self.crop = Some(crop);
        if self.shown == Some((crop, cols, rows)) {
            return None;
        }
        self.shown = Some((crop, cols, rows));
        let part = self.img.crop_imm(crop.x, crop.y, crop.w, crop.h);
        if kitty {
            let part = if part.width() > MAX_CROP_PX || part.height() > MAX_CROP_PX {
                part.resize(MAX_CROP_PX, MAX_CROP_PX, FilterType::Triangle)
            } else {
                part
            };
            let png = graphics::encode_png(&part)?;
            self.lines = graphics::placeholder_rows(ZOOM_ID, crop.cols, crop.rows).into_iter().map(Line::from).collect();
            Some(format!("{DELETE_ZOOM}{}", graphics::transmit(ZOOM_ID, &png, crop.cols, crop.rows)))
        } else {
            let exact = part.resize_exact(
                u32::try_from(crop.cols).unwrap_or(1),
                u32::try_from(crop.rows * 2).unwrap_or(2),
                FilterType::Triangle,
            );
            self.lines = halfblock(&exact, crop.cols, crop.rows);
            None
        }
    }

    /// Trata uma tecla; `false` = fechar o zoom.
    pub fn on_key(&mut self, code: KeyCode) -> bool {
        let (w, h) = (self.img.width(), self.img.height());
        let crop = self.crop;
        let pan = |v: &mut View, fx: f64, fy: f64| {
            if let Some(c) = &crop {
                v.pan(fx, fy, c);
            }
        };
        match code {
            KeyCode::Esc | KeyCode::Char('q' | 'i') => return false,
            KeyCode::Char('+' | '=') => self.view.zoom_in(self.fit, self.max),
            KeyCode::Char('-' | '_') => self.view.zoom_out(),
            KeyCode::Char('0') => self.view.reset(w, h),
            KeyCode::Char('h') | KeyCode::Left => pan(&mut self.view, -PAN_STEP, 0.0),
            KeyCode::Char('l') | KeyCode::Right => pan(&mut self.view, PAN_STEP, 0.0),
            KeyCode::Char('k') | KeyCode::Up => pan(&mut self.view, 0.0, -PAN_STEP),
            KeyCode::Char('j') | KeyCode::Down => pan(&mut self.view, 0.0, PAN_STEP),
            KeyCode::Char('b') | KeyCode::PageUp => pan(&mut self.view, 0.0, -PAGE_STEP),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => pan(&mut self.view, 0.0, PAGE_STEP),
            KeyCode::Char('g') | KeyCode::Home => self.view.set_center(None, Some(0.0)),
            KeyCode::Char('G') | KeyCode::End => self.view.set_center(None, Some(f64::from(h))),
            _ => {}
        }
        true
    }
}

impl App {
    /// `i`: abre o zoom do diagrama Mermaid com mais linhas na tela.
    pub(crate) fn open_visible_mermaid(&mut self) {
        let achado = crate::app::zoom::most_visible(self.doc.rendered.mermaids.iter().map(|m| (m.start, m.end)), self.window());
        match achado {
            Some(index) => self.open_mermaid(index),
            None => self.notice = Some("nenhum diagrama na tela".to_string()),
        }
    }

    /// Abre o zoom do diagrama `index` de `Rendered.mermaids`.
    fn open_mermaid(&mut self, index: usize) {
        let key = self.doc.rendered.mermaids[index].key.clone();
        let Some(png) = self.images.mermaid_png(&key) else {
            self.notice = Some("diagrama ainda não está pronto".to_string());
            return;
        };
        match crate::image::decode(&png) {
            Ok(img) => {
                self.notice = None;
                self.modal = Modal::ImageZoom(ImgZoom::new(index, key, img));
            }
            Err(e) => self.notice = Some(format!("não consegui abrir o diagrama: {}", crate::render::sanitize(&e))),
        }
    }

    /// Fecha o zoom se o documento mudou e o diagrama sumiu.
    pub(crate) fn sync_img_zoom(&mut self) {
        let gone = self.img_zoom().is_some_and(|z| {
            self.doc.rendered.mermaids.get(z.index).is_none_or(|m| m.key != z.key)
        });
        if gone {
            self.modal = Modal::None;
        }
    }

    /// Antes de cada quadro: recorta o diagrama para a tela e enfileira o envio ao terminal.
    pub(crate) fn prepare_img_zoom(&mut self, cols: u16, rows: u16, px_per_col: f64) {
        let kitty = self.images.graphics_enabled();
        let Some(z) = self.img_zoom_mut() else { return };
        let area_rows = usize::from(rows.saturating_sub(1)).max(1);
        if let Some(seq) = z.refresh(usize::from(cols).max(1), area_rows, px_per_col, kitty) {
            self.pending_gfx.push_str(&seq);
            self.zoom_sent = true;
        }
    }

    pub(crate) fn on_img_zoom_key(&mut self, code: KeyCode) {
        if let (KeyCode::Char(c @ ('n' | 'p')), Some(z)) = (code, self.img_zoom()) {
            if let Some(next) = crate::app::zoom::step_index(z.index, self.doc.rendered.mermaids.len(), c == 'n') {
                self.open_mermaid(next);
            }
            return;
        }
        let keep = self.img_zoom_mut().is_none_or(|z| z.on_key(code));
        if !keep {
            self.modal = Modal::None;
        }
    }

    pub(crate) fn draw_img_zoom(&mut self, f: &mut Frame, content: Rect, status: Rect) {
        let Some(z) = self.img_zoom() else { return };
        let (cols, rows) = z.shown.map_or((0, 0), |(c, _, _)| (c.cols, c.rows));
        let w = u16::try_from(cols).unwrap_or(u16::MAX).min(content.width);
        let h = u16::try_from(rows).unwrap_or(u16::MAX).min(content.height);
        let area = Rect {
            x: content.x + (content.width - w) / 2,
            y: content.y + (content.height - h) / 2,
            width: w,
            height: h,
        };
        f.render_widget(Paragraph::new(z.lines.clone()), area);

        let bar = Style::default().bg(Color::Rgb(59, 66, 82)).fg(Color::Rgb(229, 233, 240));
        let titulo = self
            .doc
            .rendered
            .mermaids
            .get(z.index)
            .and_then(|m| crate::app::zoom::section_title(&self.doc.rendered, m.start))
            .map_or(String::new(), |t| format!("  «{t}»"));
        let left = format!(
            " {}  diagrama {}/{}{titulo}  zoom {:.1}×  {}×{} px",
            self.name,
            z.index + 1,
            self.doc.rendered.mermaids.len(),
            z.factor(),
            z.img.width(),
            z.img.height()
        );
        let right = "+/- zoom  hjkl mover  n/p outro diagrama  0 ajustar  Esc voltar ";
        let width = usize::from(status.width);
        let (lw, rw) = (left.width(), right.width());
        let text = if lw + rw < width { format!("{left}{}{right}", " ".repeat(width - lw - rw)) } else { left };
        f.render_widget(Paragraph::new(Line::from(text)).style(bar), status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn img(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, 7, 255])))
    }

    fn zoom(w: u32, h: u32) -> ImgZoom {
        ImgZoom::new(0, "mermaid:x".into(), img(w, h))
    }

    #[test]
    fn meio_bloco_enche_a_area_pedida() {
        let mut z = zoom(1200, 3000);
        assert!(z.refresh(100, 40, 1.0, false).is_none());
        // ajustada: 40 linhas de altura, 32 colunas (proporção preservada)
        assert_eq!(z.lines.len(), 40);
        assert_eq!(z.lines[0].spans.len(), 32);
        assert!((z.factor() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_aproxima_e_ocupa_a_tela_toda() {
        let mut z = zoom(1200, 3000);
        z.refresh(100, 40, 1.0, false);
        for _ in 0..8 {
            z.on_key(KeyCode::Char('+'));
            z.refresh(100, 40, 1.0, false);
        }
        assert!(z.factor() > 4.0, "{}", z.factor());
        assert_eq!((z.lines.len(), z.lines[0].spans.len()), (40, 100));
    }

    #[test]
    fn deslocar_muda_o_recorte_e_g_vai_ao_topo() {
        let mut z = zoom(1200, 3000);
        for _ in 0..8 {
            z.on_key(KeyCode::Char('+'));
            z.refresh(100, 40, 1.0, false);
        }
        let antes = z.crop.unwrap();
        z.on_key(KeyCode::Char('j'));
        z.refresh(100, 40, 1.0, false);
        assert!(z.crop.unwrap().y > antes.y);
        z.on_key(KeyCode::Char('g'));
        z.refresh(100, 40, 1.0, false);
        assert_eq!(z.crop.unwrap().y, 0);
        z.on_key(KeyCode::Char('G'));
        z.refresh(100, 40, 1.0, false);
        let c = z.crop.unwrap();
        assert_eq!(c.y + c.h, 3000);
        z.on_key(KeyCode::Char('0'));
        z.refresh(100, 40, 1.0, false);
        assert!((z.factor() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn nao_redesenha_sem_mudanca() {
        let mut z = zoom(200, 100);
        z.refresh(100, 40, 8.0, true);
        assert!(z.refresh(100, 40, 8.0, true).is_none(), "mesmo recorte: nada a reenviar");
    }

    #[test]
    fn kitty_envia_o_recorte_com_o_id_do_zoom_e_limita_as_celulas() {
        let mut z = zoom(1200, 3000);
        let seq = z.refresh(300, 200, 8.0, true).expect("envia");
        assert!(seq.starts_with(DELETE_ZOOM), "{seq:.40}");
        assert!(seq.contains("i=16776961,U=1"), "{seq:.120}");
        assert!(z.lines.len() <= graphics::MAX_CELLS && z.shown.unwrap().1 <= graphics::MAX_CELLS);
    }

    #[test]
    fn id_do_zoom_bate_com_a_sequencia_de_apagar() {
        assert_eq!(ZOOM_ID, 16_776_961);
    }

    #[test]
    fn esc_q_e_i_fecham() {
        for k in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('i')] {
            assert!(!zoom(10, 10).on_key(k));
        }
        assert!(zoom(10, 10).on_key(KeyCode::Char('x')));
    }
}
