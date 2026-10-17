//! Imagens e diagramas de um documento: troca `Block::Image` e blocos ```mermaid por
//! `Block::Picture` (ou por uma nota, com aviso). A **política de caminho** é daqui; carregar a
//! imagem (disco, rede, cache) é do `PictureProvider`.

use std::path::{Component, Path};
use std::sync::Arc;

use image::{DynamicImage, ImageFormat};
use mdv_text::is_confined;

use super::model::{Block, Document, Inline, Run};
use super::PictureProvider;

/// Imagens por documento.
pub const MAX_PICTURES: usize = 30;
/// Maior lado, em pixels, de uma imagem no arquivo exportado.
pub const MAX_SIDE: u32 = 1200;

struct Resolver<'a> {
    pictures: &'a dyn PictureProvider,
    count: usize,
    warnings: Vec<String>,
}

/// Troca cada `Block::Image` e cada bloco ```mermaid pela imagem decodificada. Devolve os avisos.
pub fn resolve(doc: &mut Document, pictures: &dyn PictureProvider) -> Vec<String> {
    let mut r = Resolver { pictures, count: 0, warnings: Vec::new() };
    r.walk(&mut doc.blocks);
    r.warnings
}

impl Resolver<'_> {
    fn walk(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Image { alt, src } => {
                    let (alt, src) = (std::mem::take(alt), std::mem::take(src));
                    *block = self.picture_or_note(&alt, &src);
                }
                Block::Code { lang, text } if lang == "mermaid" => {
                    if let Some(img) = self.take_mermaid(text) {
                        *block = Block::Picture { alt: "diagrama mermaid".to_string(), image: img };
                    }
                }
                Block::Quote { blocks, .. } => self.walk(blocks),
                Block::List { items, .. } => items.iter_mut().for_each(|i| self.walk(&mut i.blocks)),
                _ => {}
            }
        }
    }

    fn picture_or_note(&mut self, alt: &str, src: &str) -> Block {
        let failure = match self.reserve() {
            Err(e) => e,
            Ok(()) if !local_path_is_safe(src) => {
                "caminho fora da pasta do documento (só valem caminhos relativos, sem ..)".to_string()
            }
            Ok(()) => match self.pictures.load(src) {
                Ok(img) => return Block::Picture { alt: alt.to_string(), image: shrink(&img) },
                Err(e) => e,
            },
        };
        self.warnings.push(format!("imagem {src}: {failure}"));
        Block::Paragraph(vec![Inline::Run(Run::plain(format!("[imagem: {alt}]")))])
    }

    fn take_mermaid(&mut self, source: &str) -> Option<Arc<DynamicImage>> {
        // `None` do provedor: Mermaid desligado, o bloco continua código e não há aviso.
        let result = self.pictures.mermaid(source)?;
        if let Err(e) = self.reserve() {
            self.warnings.push(format!("diagrama mermaid: {e}"));
            return None;
        }
        match result {
            Ok(img) => Some(shrink(&img)),
            Err(e) => {
                self.warnings.push(format!("diagrama mermaid ficou como código: {e}"));
                None
            }
        }
    }

    fn reserve(&mut self) -> Result<(), String> {
        if self.count >= MAX_PICTURES {
            return Err(format!("limite de {MAX_PICTURES} imagens por documento"));
        }
        self.count += 1;
        Ok(())
    }
}

fn is_url(src: &str) -> bool {
    let s = src.to_ascii_lowercase();
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
}

/// O arquivo exportado costuma ser compartilhado: uma imagem com caminho absoluto, com `..` ou com
/// unidade/UNC poderia levar para dentro dele um arquivo qualquer do computador de quem exporta.
/// URLs (`http`, `data:`) seguem as regras do provedor (rede só com `--remote`, no `mdv`).
fn local_path_is_safe(src: &str) -> bool {
    is_url(src) || (is_confined(src) && !Path::new(src).components().any(|c| matches!(c, Component::ParentDir)))
}

/// Reduz imagens com lado acima de `MAX_SIDE` (memória e tamanho do arquivo sob controle).
pub fn shrink(img: &Arc<DynamicImage>) -> Arc<DynamicImage> {
    if img.width().max(img.height()) > MAX_SIDE {
        Arc::new(img.thumbnail(MAX_SIDE, MAX_SIDE))
    } else {
        Arc::clone(img)
    }
}

/// PNG da imagem (DOCX e ODT guardam o arquivo como está).
pub fn to_png(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| format!("não consegui codificar a imagem: {e}"))?;
    Ok(out.into_inner())
}

/// Pixels RGB de 8 bits, linha a linha (o PDF não aceita transparência neste exportador).
pub fn to_rgb(img: &DynamicImage) -> (u32, u32, Vec<u8>) {
    let rgb = img.to_rgb8();
    (rgb.width(), rgb.height(), rgb.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FakePictures;

    fn doc_with(src: &str) -> Document {
        Document { title: None, blocks: vec![Block::Image { alt: "foto".into(), src: src.into() }] }
    }

    fn is_picture(d: &Document) -> bool {
        matches!(d.blocks[0], Block::Picture { .. })
    }

    #[test]
    fn caminho_relativo_seguro_carrega() {
        let p = FakePictures::new();
        let mut d = doc_with("img/a.png");
        let w = resolve(&mut d, &p);
        assert!(is_picture(&d) && w.is_empty(), "{w:?}");
        assert_eq!(p.loads(), ["img/a.png"]);
    }

    #[test]
    fn caminho_inseguro_nunca_chega_ao_provedor() {
        for src in ["/etc/passwd", "../segredo.png", "a/../../b.png", "C:\\x.png", "\\\\srv\\x.png", "c:/x.png"] {
            let p = FakePictures::new();
            let mut d = doc_with(src);
            let w = resolve(&mut d, &p);
            assert!(!is_picture(&d), "{src}");
            assert!(w.iter().any(|x| x.contains("fora da pasta")), "{src}: {w:?}");
            assert!(p.loads().is_empty(), "{src} não pode ser consultado");
            let Block::Paragraph(_) = &d.blocks[0] else { panic!("{src}") };
        }
    }

    #[test]
    fn url_http_e_data_passam_para_o_provedor() {
        for src in ["https://x.org/a.png", "HTTP://x.org/a.png", "data:image/png;base64,AAAA"] {
            let p = FakePictures::new();
            let mut d = doc_with(src);
            resolve(&mut d, &p);
            assert_eq!(p.loads().len(), 1, "{src}");
        }
    }

    #[test]
    fn falha_do_provedor_vira_nota_com_aviso() {
        let p = FakePictures::failing("não achei");
        let mut d = doc_with("a.png");
        let w = resolve(&mut d, &p);
        assert!(w.iter().any(|x| x.contains("a.png") && x.contains("não achei")), "{w:?}");
        assert!(!is_picture(&d));
    }

    #[test]
    fn imagem_grande_e_reduzida_ao_maior_lado() {
        let p = FakePictures::with_size(3000, 1500);
        let mut d = doc_with("a.png");
        resolve(&mut d, &p);
        let Block::Picture { image, .. } = &d.blocks[0] else { panic!() };
        assert_eq!(image.width().max(image.height()), MAX_SIDE);
    }

    #[test]
    fn teto_de_imagens_por_documento() {
        let p = FakePictures::new();
        let mut d = Document {
            title: None,
            blocks: (0..MAX_PICTURES + 2).map(|i| Block::Image { alt: "x".into(), src: format!("{i}.png") }).collect(),
        };
        let w = resolve(&mut d, &p);
        assert_eq!(p.loads().len(), MAX_PICTURES);
        assert_eq!(w.iter().filter(|x| x.contains("limite")).count(), 2, "{w:?}");
    }

    #[test]
    fn mermaid_vira_imagem_so_se_o_provedor_o_desenha() {
        let code = || Document { title: None, blocks: vec![Block::Code { lang: "mermaid".into(), text: "graph TD; A-->B".into() }] };
        let mut d = code();
        let w = resolve(&mut d, &FakePictures::new().with_mermaid());
        assert!(is_picture(&d) && w.is_empty(), "{w:?}");
        let mut d = code();
        let w = resolve(&mut d, &FakePictures::new());
        assert!(matches!(d.blocks[0], Block::Code { .. }) && w.is_empty(), "Mermaid desligado: só código, sem aviso");
        let mut d = code();
        let w = resolve(&mut d, &FakePictures::new().with_failing_mermaid("quebrou"));
        assert!(matches!(d.blocks[0], Block::Code { .. }));
        assert!(w.iter().any(|x| x.contains("quebrou")), "{w:?}");
    }

    #[test]
    fn imagens_aninhadas_em_citacao_e_lista_tambem_sao_resolvidas() {
        let p = FakePictures::new();
        let mut d = Document {
            title: None,
            blocks: vec![Block::Quote { title: None, blocks: vec![Block::Image { alt: "q".into(), src: "q.png".into() }] }],
        };
        resolve(&mut d, &p);
        assert_eq!(p.loads(), ["q.png"]);
    }
}
