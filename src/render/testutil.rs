//! Auxiliares compartilhados pelos testes de `render`.

use super::*;

pub(super) const ESC: char = '\x1b';

/// Remove somente sequências SGR bem formadas (`ESC [ dígitos;… m`), as que o `to_ansi` gera.
pub(super) fn strip_sgr(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(pos) = rest.find(ESC) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let sgr_len = after
            .strip_prefix('[')
            .and_then(|p| p.find(|c: char| !(c.is_ascii_digit() || c == ';')).map(|n| (p, n)))
            .filter(|(p, n)| p[*n..].starts_with('m'))
            .map(|(_, n)| n + 2);
        match sgr_len {
            Some(n) => rest = &after[n..],
            None => {
                out.push(ESC);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub(super) fn plain_output(md: &str) -> String {
    to_ansi(&render(md, 80).lines, true)
}

pub(super) fn rendered_text(md: &str) -> Vec<String> {
    rendered_text_at(md, 80)
}

pub(super) fn rendered_text_at(md: &str, width: usize) -> Vec<String> {
    render(md, width)
        .lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

pub(super) fn assert_fits(md: &str, width: usize) {
    for (i, l) in render(md, width).lines.iter().enumerate() {
        let text: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(l.width() <= width, "linha {i} com {} colunas (max {width}): {text:?}", l.width());
    }
}

pub(super) fn joined_text(r: &Rendered) -> String {
    r.lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn png_no_dir(dir: &std::path::Path, nome: &str) {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])));
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    std::fs::write(dir.join(nome), png.into_inner()).unwrap();
}

pub(super) fn t60(md: &str) -> String {
    joined_text(&render(md, 60))
}
