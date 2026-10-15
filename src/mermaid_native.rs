//! Renderizador Mermaid nativo (sem Chromium): `mermaid-rs-renderer` gera o SVG e `resvg` o rasteriza.

use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};

type Fonts = Arc<usvg::fontdb::Database>;

/// Fontes do sistema, carregadas uma única vez (carregar custa dezenas de ms).
fn system_fonts() -> &'static Fonts {
    static DB: OnceLock<Fonts> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
}

/// Desenha o diagrama Mermaid e devolve um PNG.
pub fn render_png(source: &str) -> Result<Vec<u8>, String> {
    let svg = mermaid_rs_renderer::render(source)
        .map_err(|e| e.to_string().lines().next().unwrap_or("erro").to_string())?;
    let opt = usvg::Options { fontdb: Arc::clone(system_fonts()), ..Default::default() };
    let tree = usvg::Tree::from_str(&svg, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("não consegui alocar a imagem")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOW: &str = "flowchart LR\n  A[Início] --> B{Decide?}\n  B -->|sim| C[Faz]\n  B -->|não| D[Para]\n";

    fn dims(png: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(png).expect("PNG válido");
        (img.width(), img.height())
    }

    #[test]
    fn fluxograma_vira_png() {
        let png = render_png(FLOW).expect("deveria renderizar");
        assert_eq!(&png[1..4], b"PNG");
        let (w, h) = dims(&png);
        assert!(w > 50 && h > 20, "{w}x{h}");
    }

    #[test]
    fn sintaxe_invalida_vira_erro_em_portugues_ou_do_crate() {
        let e = render_png("isto não é mermaid").unwrap_err();
        assert!(!e.trim().is_empty());
    }
}
