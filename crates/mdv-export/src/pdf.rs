//! Escritor PDF próprio: serialização (objetos, `xref`) do que `pdf_layout` posicionou.
//! Só usa as fontes padrão do formato (ver `pdf_font`), sem incorporar fontes.

use super::images::to_rgb;
use super::model::Document;
use super::pdf_font::Face;
use super::pdf_layout::{Layout, Op, MARGIN, PAGE_H, PAGE_W};
use super::{Budget, Exporter, Output};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

/// Teto do arquivo gerado.
pub const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

pub struct PdfExporter;

impl Exporter for PdfExporter {
    fn write(&self, doc: &Document, budget: &dyn Budget) -> Result<Output, String> {
        let mut l = Layout::new(budget);
        for b in &doc.blocks {
            l.block(b, MARGIN, PAGE_W - 2.0 * MARGIN)?;
        }
        let warnings = l.warnings();
        let bytes = l.serialize(doc.title.as_deref())?;
        if bytes.len() > MAX_PDF_BYTES {
            return Err(format!("PDF maior que {} MiB", MAX_PDF_BYTES / (1024 * 1024)));
        }
        Ok(Output { bytes, warnings })
    }
}

impl Layout<'_> {
    fn serialize(&self, title: Option<&str>) -> Result<Vec<u8>, String> {
        let n_images = self.images.len();
        let first_image = 9;
        let first_page = first_image + n_images;
        let total_objs = first_page + 2 * self.pages.len();
        let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets: Vec<usize> = Vec::with_capacity(total_objs);
        let begin = |out: &mut Vec<u8>, offsets: &mut Vec<usize>| {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        };

        begin(&mut out, &mut offsets);
        out.extend_from_slice(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        begin(&mut out, &mut offsets);
        let kids: Vec<String> = (0..self.pages.len()).map(|i| format!("{} 0 R", first_page + 2 * i + 1)).collect();
        out.extend_from_slice(format!("<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n", kids.join(" "), self.pages.len()).as_bytes());
        for face in Face::ALL {
            begin(&mut out, &mut offsets);
            out.extend_from_slice(
                format!("<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>\nendobj\n", face.base_font()).as_bytes(),
            );
        }
        begin(&mut out, &mut offsets);
        let utf16: String = title.unwrap_or("").encode_utf16().map(|u| format!("{u:04X}")).collect();
        out.extend_from_slice(format!("<< /Producer (mdv) /Title <FEFF{utf16}> >>\nendobj\n").as_bytes());

        for img in &self.images {
            let (w, h, rgb) = to_rgb(img);
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&rgb).map_err(|e| e.to_string())?;
            let data = enc.finish().map_err(|e| e.to_string())?;
            begin(&mut out, &mut offsets);
            out.extend_from_slice(
                format!(
                    "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                    data.len()
                )
                .as_bytes(),
            );
            out.extend_from_slice(&data);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        }

        let fonts: String = Face::ALL.iter().enumerate().map(|(i, f)| format!("/{} {} 0 R ", f.resource(), 3 + i)).collect();
        let xobjects: String = (0..n_images).map(|i| format!("/Im{i} {} 0 R ", first_image + i)).collect();
        for (i, ops) in self.pages.iter().enumerate() {
            let content = page_content(ops);
            begin(&mut out, &mut offsets);
            out.extend_from_slice(format!("<< /Length {} >>\nstream\n", content.len()).as_bytes());
            out.extend_from_slice(&content);
            out.extend_from_slice(b"\nendstream\nendobj\n");
            begin(&mut out, &mut offsets);
            out.extend_from_slice(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << {fonts}>> /XObject << {xobjects}>> >> /Contents {} 0 R >>\nendobj\n",
                    first_page + 2 * i
                )
                .as_bytes(),
            );
        }

        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 8 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
        Ok(out)
    }
}

/// Texto entre parênteses do PDF: `(`, `)` e `\` com barra; fora do ASCII visível em octal.
fn pdf_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            32..=126 => out.push(b),
            _ => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
        }
    }
    out
}

fn page_content(ops: &[Op]) -> Vec<u8> {
    let mut s: Vec<u8> = Vec::new();
    for op in ops {
        match op {
            Op::Text { x, y, face, size, color, bytes } => {
                s.extend_from_slice(
                    format!(
                        "BT /{} {size:.2} Tf {:.3} {:.3} {:.3} rg {x:.2} {:.2} Td (",
                        face.resource(),
                        color.0,
                        color.1,
                        color.2,
                        PAGE_H - y
                    )
                    .as_bytes(),
                );
                s.extend_from_slice(&pdf_string(bytes));
                s.extend_from_slice(b") Tj ET\n");
            }
            Op::Rect { x, top, w, h, gray } => {
                s.extend_from_slice(format!("{gray:.3} g {x:.2} {:.2} {w:.2} {h:.2} re f\n", PAGE_H - top - h).as_bytes());
            }
            Op::Line { x1, y1, x2, y2, gray, width } => {
                s.extend_from_slice(
                    format!("{width:.2} w {gray:.3} G {x1:.2} {:.2} m {x2:.2} {:.2} l S\n", PAGE_H - y1, PAGE_H - y2).as_bytes(),
                );
            }
            Op::Image { index, x, top, w, h } => {
                s.extend_from_slice(format!("q {w:.2} 0 0 {h:.2} {x:.2} {:.2} cm /Im{index} Do Q\n", PAGE_H - top - h).as_bytes());
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Block;
    use crate::parse::parse;
    use image::DynamicImage;
    use std::sync::Arc;

    fn export(md: &str) -> Output {
        PdfExporter.write(&parse(md, &crate::Unlimited).unwrap(), &crate::Unlimited).unwrap()
    }

    fn contains(bytes: &[u8], needle: &str) -> bool {
        bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn estrutura_basica_do_pdf() {
        let o = export("# Título\n\nUm parágrafo simples.\n");
        assert!(o.bytes.starts_with(b"%PDF-1.4"));
        assert!(o.bytes.ends_with(b"%%EOF\n"));
        assert!(contains(&o.bytes, "/Type /Catalog") && contains(&o.bytes, "/Count 1"));
        assert!(contains(&o.bytes, "/BaseFont /Helvetica-Bold"));
        assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    }

    #[test]
    fn xref_aponta_para_cada_objeto() {
        let o = export("texto\n");
        let text = String::from_utf8_lossy(&o.bytes).into_owned();
        let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(o.bytes[start..].starts_with(b"xref\n0 "));
        let xref = &text[start..];
        for (n, line) in xref.lines().skip(3).take_while(|l| l.ends_with(" n ")).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(o.bytes[off..].starts_with(format!("{} 0 obj", n + 1).as_bytes()), "objeto {}", n + 1);
        }
    }

    #[test]
    fn caractere_fora_do_latin1_vira_interrogacao_com_aviso() {
        let o = export("seta → e emoji 😀\n");
        assert_eq!(o.warnings.len(), 1);
        assert!(o.warnings[0].contains('→') && o.warnings[0].contains('😀'), "{:?}", o.warnings);
        assert!(contains(&o.bytes, "(?) Tj"), "cada palavra é um Tj; o ? ocupa o lugar da seta");
    }

    #[test]
    fn acentos_saem_em_octal_winansi() {
        let o = export("ação\n");
        assert!(contains(&o.bytes, "a\\347\\343o"), "ç=347, ã=343");
    }

    #[test]
    fn texto_longo_vira_varias_paginas() {
        let md = "linha de texto\n\n".repeat(400);
        let o = export(&md);
        let pages = String::from_utf8_lossy(&o.bytes).matches("/Type /Page ").count();
        assert!(pages >= 3, "{pages}");
    }

    #[test]
    fn palavra_gigante_e_partida_sem_travar() {
        let md = format!("{}\n", "x".repeat(5000));
        let o = export(&md);
        assert!(contains(&o.bytes, "xxxxxxxx"));
    }

    #[test]
    fn lista_tabela_codigo_e_citacao_nao_falham() {
        let md = "- a\n  - b\n- [x] c\n\n1. um\n\n| h1 | h2 |\n|:--|--:|\n| a | b |\n\n```\nfn x() {}\n```\n\n> [!NOTE]\n> oi\n\n---\n";
        let o = export(md);
        assert!(contains(&o.bytes, "re f"), "fundos e barras");
        assert!(contains(&o.bytes, " l S"), "bordas da tabela");
    }

    #[test]
    fn imagem_vira_xobject_comprimido() {
        let img = Arc::new(DynamicImage::ImageRgb8(image::RgbImage::new(20, 10)));
        let doc = Document { title: Some("T".into()), blocks: vec![Block::Picture { alt: "f".into(), image: img }] };
        let o = PdfExporter.write(&doc, &crate::Unlimited).unwrap();
        assert!(contains(&o.bytes, "/Subtype /Image") && contains(&o.bytes, "/FlateDecode"));
        assert!(contains(&o.bytes, "/Im0 Do"));
    }

    #[test]
    fn titulo_vai_em_utf16_no_info() {
        let doc = Document { title: Some("Aé".into()), blocks: vec![] };
        let o = PdfExporter.write(&doc, &crate::Unlimited).unwrap();
        assert!(contains(&o.bytes, "/Title <FEFF004100E9>"));
    }
}
