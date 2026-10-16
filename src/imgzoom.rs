//! Zoom de imagem (diagramas Mermaid): contas puras de escala e janela de recorte.
//!
//! A escala é em *células por pixel da imagem* (largura). A célula tem o dobro da altura da
//! largura (a mesma suposição do meio-bloco e de `graphics::cell_size`), então a altura visível
//! em pixels da imagem é `2 * linhas / escala`.

/// Cada passo de zoom multiplica a escala por este fator.
const STEP: f64 = 1.25;
/// Nível máximo, só para o contador nunca crescer sem fim.
const MAX_LEVEL: i32 = 80;

/// Recorte da imagem (em pixels) e quantas células ele ocupa na tela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub cols: usize,
    pub rows: usize,
}

/// Escala que faz a imagem inteira caber em `cols`×`rows` células (sem ampliar além disso).
pub fn fit_scale(w: u32, h: u32, cols: usize, rows: usize) -> f64 {
    let (w, h) = (f64::from(w.max(1)), f64::from(h.max(1)));
    (cols.max(1) as f64 / w).min(2.0 * rows.max(1) as f64 / h)
}

/// Maior escala útil: 2 células por pixel no meio-bloco (`px_per_col` = 1) ou 2 pixels do
/// terminal por pixel da imagem no Kitty. Nunca menor que o ajuste à tela.
pub fn max_scale(fit: f64, px_per_col: f64) -> f64 {
    fit.max(2.0 / px_per_col.max(0.1))
}

fn scale_for(level: i32, fit: f64, max: f64) -> f64 {
    (fit * STEP.powi(level)).min(max)
}

/// Estado do zoom: nível e centro da janela, em pixels da imagem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub level: i32,
    pub cx: f64,
    pub cy: f64,
}

impl View {
    /// Começa mostrando a imagem inteira, centrada.
    pub fn new(w: u32, h: u32) -> Self {
        View { level: 0, cx: f64::from(w) / 2.0, cy: f64::from(h) / 2.0 }
    }

    pub fn scale(&self, fit: f64, max: f64) -> f64 {
        scale_for(self.level, fit, max)
    }

    /// Mais zoom, até `max`.
    pub fn zoom_in(&mut self, fit: f64, max: f64) {
        if self.level < MAX_LEVEL && scale_for(self.level, fit, max) < max {
            self.level += 1;
        }
    }

    /// Menos zoom, até a imagem inteira.
    pub fn zoom_out(&mut self) {
        self.level = (self.level - 1).max(0);
    }

    /// Volta a mostrar a imagem inteira.
    pub fn reset(&mut self, w: u32, h: u32) {
        *self = View::new(w, h);
    }

    /// Desloca a janela por frações do recorte atual (`fx` da largura, `fy` da altura).
    pub fn pan(&mut self, fx: f64, fy: f64, frame: &Frame) {
        self.cx += fx * f64::from(frame.w);
        self.cy += fy * f64::from(frame.h);
    }

    /// Guarda o centro já limitado à imagem, para o próximo deslocamento partir dele.
    pub fn settle(&mut self, frame: &Frame) {
        self.cx = f64::from(frame.x) + f64::from(frame.w) / 2.0;
        self.cy = f64::from(frame.y) + f64::from(frame.h) / 2.0;
    }

    /// Vai para o topo/fim ou para a esquerda/direita, mantendo o outro eixo.
    pub fn set_center(&mut self, cx: Option<f64>, cy: Option<f64>) {
        if let Some(v) = cx {
            self.cx = v;
        }
        if let Some(v) = cy {
            self.cy = v;
        }
    }
}

/// Recorte de uma imagem `w`×`h` que cabe em `cols`×`rows` células na `scale`, centrado em
/// (`cx`, `cy`) e sempre dentro da imagem.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn frame(w: u32, h: u32, cols: usize, rows: usize, scale: f64, cx: f64, cy: f64) -> Frame {
    let (wf, hf) = (f64::from(w.max(1)), f64::from(h.max(1)));
    let scale = scale.max(f64::MIN_POSITIVE);
    let vw = (cols.max(1) as f64 / scale).min(wf);
    let vh = (2.0 * rows.max(1) as f64 / scale).min(hf);
    let x = (cx - vw / 2.0).clamp(0.0, wf - vw);
    let y = (cy - vh / 2.0).clamp(0.0, hf - vh);
    let used_cols = ((vw * scale).round() as usize).clamp(1, cols.max(1));
    let used_rows = ((vh * scale / 2.0).round() as usize).clamp(1, rows.max(1));
    Frame {
        x: x.round() as u32,
        y: y.round() as u32,
        w: (vw.round() as u32).clamp(1, w.max(1)),
        h: (vh.round() as u32).clamp(1, h.max(1)),
        cols: used_cols,
        rows: used_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ajuste_usa_o_eixo_mais_apertado() {
        // 1200×3000 em 100×40 células: a altura (80 meias-linhas) manda.
        let fit = fit_scale(1200, 3000, 100, 40);
        assert!((fit - 80.0 / 3000.0).abs() < 1e-9, "{fit}");
        // largo e baixo: a largura manda.
        let fit = fit_scale(1000, 100, 100, 40);
        assert!((fit - 0.1).abs() < 1e-9, "{fit}");
    }

    #[test]
    fn no_ajuste_o_recorte_e_a_imagem_inteira() {
        let fit = fit_scale(1200, 3000, 100, 40);
        let f = frame(1200, 3000, 100, 40, fit, 600.0, 1500.0);
        assert_eq!((f.x, f.y, f.w, f.h), (0, 0, 1200, 3000));
        assert_eq!((f.cols, f.rows), (32, 40));
    }

    #[test]
    fn com_zoom_o_recorte_encolhe_e_segue_o_centro() {
        let f = frame(1200, 3000, 100, 40, 0.5, 600.0, 1500.0);
        assert_eq!((f.w, f.h), (200, 160));
        assert_eq!((f.x, f.y), (500, 1420));
        assert_eq!((f.cols, f.rows), (100, 40));
    }

    #[test]
    fn recorte_nunca_sai_da_imagem() {
        for (cx, cy) in [(-500.0, -500.0), (0.0, 0.0), (1200.0, 3000.0), (9e9, 9e9)] {
            let f = frame(1200, 3000, 100, 40, 0.5, cx, cy);
            assert!(f.x + f.w <= 1200 && f.y + f.h <= 3000, "{f:?}");
        }
        let f = frame(1200, 3000, 100, 40, 0.5, 9e9, 9e9);
        assert_eq!((f.x, f.y), (1000, 2840));
    }

    #[test]
    fn imagem_menor_que_a_janela_nao_e_ampliada_alem_da_escala() {
        // 40×20 px a 1 célula/px: ocupa 40×10 células.
        let f = frame(40, 20, 100, 40, 1.0, 20.0, 10.0);
        assert_eq!((f.x, f.y, f.w, f.h), (0, 0, 40, 20));
        assert_eq!((f.cols, f.rows), (40, 10));
    }

    #[test]
    fn zoom_para_no_maximo_e_no_ajuste() {
        let fit = fit_scale(1200, 3000, 100, 40);
        let max = max_scale(fit, 1.0);
        assert!((max - 2.0).abs() < 1e-9);
        let mut v = View::new(1200, 3000);
        for _ in 0..200 {
            v.zoom_in(fit, max);
        }
        assert!((v.scale(fit, max) - max).abs() < 1e-9);
        let nivel = v.level;
        v.zoom_in(fit, max);
        assert_eq!(v.level, nivel, "no máximo não sobe mais");
        for _ in 0..200 {
            v.zoom_out();
        }
        assert_eq!(v.level, 0);
        assert!((v.scale(fit, max) - fit).abs() < 1e-9);
    }

    #[test]
    fn maximo_nunca_fica_abaixo_do_ajuste() {
        // imagem minúscula: o ajuste (200) passa de 2.
        assert!((max_scale(200.0, 1.0) - 200.0).abs() < 1e-9);
        // Kitty com células de 10 px: 0,2 célula por pixel.
        assert!((max_scale(0.02, 10.0) - 0.2).abs() < 1e-9);
    }

    #[test]
    fn deslocar_e_assentar_mantem_o_centro_dentro_da_imagem() {
        let fit = fit_scale(1200, 3000, 100, 40);
        let mut v = View::new(1200, 3000);
        v.level = 12;
        let sc = v.scale(fit, 2.0);
        let f = frame(1200, 3000, 100, 40, sc, v.cx, v.cy);
        v.pan(10.0, 10.0, &f);
        let f2 = frame(1200, 3000, 100, 40, sc, v.cx, v.cy);
        v.settle(&f2);
        assert!(v.cx <= 1200.0 && v.cy <= 3000.0, "{v:?}");
        let f3 = frame(1200, 3000, 100, 40, sc, v.cx, v.cy);
        assert_eq!(f2, f3, "assentar não muda o recorte");
    }
}
