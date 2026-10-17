//! Imagens no renderizador: o `trait ImageProvider` (o que o `Renderer` precisa do `ImageStore`)
//! e o desenho de figuras, inclusive as de célula de tabela e os diagramas Mermaid.

use super::*;

/// O que o `Renderer` consulta para desenhar imagens. O `ImageStore` o implementa; os testes
/// usam um fake. Sem I/O aqui: quem busca arquivo ou rede é o provedor.
pub trait ImageProvider {
    fn graphics_enabled(&self) -> bool;
    fn graphic(&self, img: &Arc<image::DynamicImage>, cols: usize, rows: usize) -> Option<Graphic>;
    fn mermaid_enabled(&self) -> bool;
    /// Imagem de `src` (arquivo, URL ou `data:`).
    fn image(&self, src: &str) -> ImageState;
    /// Diagrama Mermaid de `source`.
    fn mermaid(&self, source: &str) -> ImageState;
    fn mermaid_key(&self, source: &str) -> String;
}

impl ImageProvider for ImageStore {
    fn graphics_enabled(&self) -> bool {
        ImageStore::graphics_enabled(self)
    }
    fn graphic(&self, img: &Arc<image::DynamicImage>, cols: usize, rows: usize) -> Option<Graphic> {
        ImageStore::graphic(self, img, cols, rows)
    }
    fn mermaid_enabled(&self) -> bool {
        ImageStore::mermaid_enabled(self)
    }
    fn image(&self, src: &str) -> ImageState {
        self.get(src)
    }
    fn mermaid(&self, source: &str) -> ImageState {
        self.get_mermaid(source)
    }
    fn mermaid_key(&self, source: &str) -> String {
        ImageStore::mermaid_key(source)
    }
}

pub(super) struct ImageCtx {
    pub(super) url: String,
    pub(super) mark: usize,
}

impl Renderer<'_> {
    pub(super) fn end_image(&mut self) {
        self.inline.pop_style();
        let in_table = self.table.is_some();
        let state = self
            .image
            .take()
            .zip(self.images)
            .map(|(ctx, store)| (ctx.mark, store.image(&ctx.url)));
        match state {
            // Em célula não há como intercalar linhas de imagem: a célula ganha `↓n` e a
            // figura é desenhada depois da tabela (`flush_table_figures`).
            Some((mark, ImageState::Ready(img))) if in_table => {
                let from = (mark + 1).min(self.inline.pieces.len());
                let alt: String = self.inline.pieces[from..].iter().map(|p| p.as_text().0).collect();
                self.table_figures.push((alt, img));
                let n = self.table_figures.len();
                self.inline.push_dim(format!(" ↓{n}]"));
            }
            Some((mark, ImageState::Ready(img))) => {
                self.inline.pieces.truncate(mark);
                self.flush_inline();
                self.push_image(&img);
            }
            Some((_, ImageState::Loading)) => self.inline.push_dim(" … carregando]"),
            Some((_, ImageState::Failed(e))) => self.inline.push_dim(format!(" — {}]", sanitize(&e))),
            None => self.inline.push_dim("]"),
        }
    }

    /// Imagens que estavam em células: a tabela só tem o `↓n`; aqui vêm elas, numeradas, cada uma
    /// com uma legenda de uma linha (cortada na largura) e o desenho.
    pub(super) fn flush_table_figures(&mut self) {
        let figures = std::mem::take(&mut self.table_figures);
        let dim = Style::default().fg(DIM);
        for (i, (alt, img)) in figures.into_iter().enumerate() {
            let (_, cont) = self.prefixes();
            let alt = alt.trim();
            let caption = if alt.is_empty() { format!("↓{}", i + 1) } else { format!("↓{} {alt}", i + 1) };
            let first_row = split_by_width(&[(caption, dim)], self.content_width())
                .into_iter()
                .next()
                .unwrap_or_default();
            let mut v = cont;
            v.extend(first_row.into_iter().map(|(t, st)| Span::styled(t, st)));
            self.out.lines.push(Line::from(v));
            self.push_image(&img);
        }
    }

    pub(super) fn push_image(&mut self, img: &Arc<image::DynamicImage>) {
        let (first, cont) = self.prefixes();
        self.take_marker();
        let max_cols = self.width.saturating_sub(spans_width(&first)).clamp(1, MAX_IMAGE_COLS);
        if self.images.is_some_and(|s| s.graphics_enabled()) {
            let (cols, rows) = graphics::cell_size(img.width(), img.height(), max_cols, MAX_IMAGE_ROWS);
            if let Some(g) = self.images.and_then(|s| s.graphic(img, cols, rows)) {
                for (i, span) in graphics::placeholder_rows(g.id, cols, rows).into_iter().enumerate() {
                    let mut v = if i == 0 { first.clone() } else { cont.clone() };
                    v.push(span);
                    self.out.lines.push(Line::from(v));
                }
                if !self.out.graphics.iter().any(|x| x.id == g.id) {
                    self.out.graphics.push(GraphicRef { id: g.id, cols, rows, png: g.png });
                }
                return;
            }
        }
        for (i, line) in halfblock(img, max_cols, MAX_IMAGE_ROWS).into_iter().enumerate() {
            let mut v = if i == 0 { first.clone() } else { cont.clone() };
            v.extend(line.spans);
            self.out.lines.push(Line::from(v));
        }
    }

    /// `None`: o diagrama virou figura. `Some(rótulo)`: segue como bloco de código com esse rótulo.
    pub(super) fn mermaid_figure(&mut self, block: &CodeBlock) -> Option<String> {
        let mut label = block.lang.clone();
        if block.lang == "mermaid" {
            if let Some(store) = self.images.filter(|s| s.mermaid_enabled()) {
                match store.mermaid(&block.buf) {
                    ImageState::Ready(img) => {
                        let start = self.out.lines.len();
                        self.push_image(&img);
                        let key = store.mermaid_key(&block.buf);
                        self.out.mermaids.push(MermaidSpan { start, end: self.out.lines.len(), key });
                        return None;
                    }
                    ImageState::Loading => label = "mermaid (renderizando…)".into(),
                    ImageState::Failed(e) => label = format!("mermaid (erro: {})", sanitize(&e)),
                }
            }
        }
        Some(label)
    }
}
