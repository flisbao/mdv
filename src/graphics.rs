//! Imagens em alta resolução pelo protocolo gráfico do Kitty, com *placeholders Unicode*.
//!
//! A imagem é transmitida uma vez (`transmit`) e depois aparece onde o texto tiver células
//! `PLACEHOLDER` + diacrítico da linha + diacrítico da coluna, com a cor de frente igual ao `id`.
//! Nada aqui faz I/O: quem escreve no terminal é `main.rs`.

use std::fmt::Write as _;
use std::io::Cursor;

use image::DynamicImage;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::clip::base64;

/// Caractere que o terminal substitui pela fatia da imagem.
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Apaga todas as imagens do terminal (enviado ao sair).
pub const DELETE_ALL: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

/// Tamanho de cada pedaço de base64 (múltiplo de 4, como o protocolo exige).
const CHUNK: usize = 4096;

/// Primeiros 128 diacríticos de `rowcolumn-diacritics.txt` do Kitty: o índice é o número da
/// linha/coluna. O maior bloco usado é `MAX_IMAGE_COLS` (80), então 128 sobram.
const DIACRITICS: [u32; 128] = [
    0x0305, 0x030D, 0x030E, 0x0310, 0x0312, 0x033D, 0x033E, 0x033F, 0x0346, 0x034A, 0x034B, 0x034C,
    0x0350, 0x0351, 0x0352, 0x0357, 0x035B, 0x0363, 0x0364, 0x0365, 0x0366, 0x0367, 0x0368, 0x0369,
    0x036A, 0x036B, 0x036C, 0x036D, 0x036E, 0x036F, 0x0483, 0x0484, 0x0485, 0x0486, 0x0487, 0x0592,
    0x0593, 0x0594, 0x0595, 0x0597, 0x0598, 0x0599, 0x059C, 0x059D, 0x059E, 0x059F, 0x05A0, 0x05A1,
    0x05A8, 0x05A9, 0x05AB, 0x05AC, 0x05AF, 0x05C4, 0x0610, 0x0611, 0x0612, 0x0613, 0x0614, 0x0615,
    0x0616, 0x0617, 0x0657, 0x0658, 0x0659, 0x065A, 0x065B, 0x065D, 0x065E, 0x06D6, 0x06D7, 0x06D8,
    0x06D9, 0x06DA, 0x06DB, 0x06DC, 0x06DF, 0x06E0, 0x06E1, 0x06E2, 0x06E4, 0x06E7, 0x06E8, 0x06EB,
    0x06EC, 0x0730, 0x0732, 0x0733, 0x0735, 0x0736, 0x073A, 0x073D, 0x073F, 0x0740, 0x0741, 0x0743,
    0x0745, 0x0747, 0x0749, 0x074A, 0x07EB, 0x07EC, 0x07ED, 0x07EE, 0x07EF, 0x07F0, 0x07F1, 0x07F3,
    0x0816, 0x0817, 0x0818, 0x0819, 0x081B, 0x081C, 0x081D, 0x081E, 0x081F, 0x0820, 0x0821, 0x0822,
    0x0823, 0x0825, 0x0826, 0x0827, 0x0829, 0x082A, 0x082B, 0x082C,
];

fn diacritic(i: usize) -> char {
    let cp = DIACRITICS[i.min(DIACRITICS.len() - 1)];
    char::from_u32(cp).unwrap_or('\u{0305}')
}

/// Colunas × linhas de células para uma imagem de `w`×`h` px dentro de `max_cols`×`max_rows`.
/// Supõe célula com o dobro da altura da largura (a mesma conta do meio-bloco) e nunca amplia.
pub fn cell_size(w: u32, h: u32, max_cols: usize, max_rows: usize) -> (usize, usize) {
    let (w, h) = (u64::from(w.max(1)), u64::from(h.max(1)));
    let max_cols = u64::try_from(max_cols.max(1)).unwrap_or(u64::MAX);
    let max_rows = u64::try_from(max_rows.max(1)).unwrap_or(u64::MAX);
    let max_half = max_rows.saturating_mul(2);
    let cols = w.min(max_cols).min(w * max_half / h).max(1);
    let half_rows = (h * cols).div_ceil(w);
    let rows = half_rows.div_ceil(2).clamp(1, max_rows);
    (
        usize::try_from(cols).unwrap_or(usize::MAX),
        usize::try_from(rows).unwrap_or(usize::MAX),
    )
}

/// Uma linha de texto por linha da imagem; a cor de frente carrega o `id` (R,G,B = 24 bits).
pub fn placeholder_rows(id: u32, cols: usize, rows: usize) -> Vec<Span<'static>> {
    let color = Color::Rgb(
        u8::try_from((id >> 16) & 0xFF).unwrap_or(0),
        u8::try_from((id >> 8) & 0xFF).unwrap_or(0),
        u8::try_from(id & 0xFF).unwrap_or(0),
    );
    (0..rows)
        .map(|r| {
            let mut s = String::with_capacity(cols * 12);
            for c in 0..cols {
                s.push(PLACEHOLDER);
                s.push(diacritic(r));
                s.push(diacritic(c));
            }
            Span::styled(s, Style::default().fg(color))
        })
        .collect()
}

/// Sequência APC que transmite `png` (base64, em pedaços) e cria o posicionamento virtual
/// `cols`×`rows` do `id`. `q=2` silencia as respostas do terminal.
pub fn transmit(id: u32, png: &[u8], cols: usize, rows: usize) -> String {
    let b64 = base64(png);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(CHUNK).collect();
    let mut out = String::with_capacity(b64.len() + 64 * chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        // base64 é ASCII puro: qualquer corte em bytes é UTF-8 válido.
        let data = std::str::from_utf8(chunk).unwrap_or("");
        if i == 0 {
            let _ = write!(out, "\x1b_Ga=T,f=100,t=d,i={id},U=1,c={cols},r={rows},q=2,m={more};{data}\x1b\\");
        } else {
            let _ = write!(out, "\x1b_Gq=2,m={more};{data}\x1b\\");
        }
    }
    out
}

/// Codifica a imagem como PNG (formato aceito por `f=100`).
pub fn encode_png(img: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// O terminal entende placeholders Unicode do Kitty? Kitty e Ghostty sim; `tmux` e `screen`
/// engolem a sequência, então ficam de fora. `var` lê variáveis de ambiente (injetável em teste).
pub fn supports_kitty(var: impl Fn(&str) -> Option<String>) -> bool {
    if var("TMUX").is_some() || var("STY").is_some() {
        return false;
    }
    let term = var("TERM").unwrap_or_default();
    let program = var("TERM_PROGRAM").unwrap_or_default();
    term.contains("kitty")
        || term.contains("ghostty")
        || var("KITTY_WINDOW_ID").is_some()
        || program.eq_ignore_ascii_case("ghostty")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_string())
    }

    #[test]
    fn cell_size_reduz_para_caber_e_preserva_a_proporcao() {
        assert_eq!(cell_size(640, 480, 80, 24), (64, 24));
        assert_eq!(cell_size(1000, 10, 80, 24), (80, 1));
        assert_eq!(cell_size(10, 1000, 80, 24), (1, 24));
    }

    #[test]
    fn cell_size_nao_amplia_e_nao_zera() {
        assert_eq!(cell_size(4, 4, 80, 24), (4, 2));
        assert_eq!(cell_size(0, 0, 0, 0), (1, 1));
    }

    #[test]
    fn placeholders_tem_celulas_de_tres_caracteres_e_a_cor_do_id() {
        let rows = placeholder_rows(0x01_02_03, 3, 2);
        assert_eq!(rows.len(), 2);
        let chars: Vec<char> = rows[1].content.chars().collect();
        assert_eq!(chars.len(), 9); // 3 células × (placeholder + linha + coluna)
        assert_eq!(chars[0], PLACEHOLDER);
        assert_eq!(chars[1], char::from_u32(0x030D).unwrap()); // linha 1
        assert_eq!(chars[2], char::from_u32(0x0305).unwrap()); // coluna 0
        assert_eq!(chars[5], char::from_u32(0x030D).unwrap()); // coluna 1
        assert_eq!(rows[0].style.fg, Some(Color::Rgb(1, 2, 3)));
    }

    #[test]
    fn tabela_de_diacriticos_tem_128_entradas_do_kitty() {
        assert_eq!(DIACRITICS.len(), 128);
        assert_eq!((DIACRITICS[0], DIACRITICS[127]), (0x0305, 0x082C));
    }

    #[test]
    fn transmit_pequeno_cabe_num_pedaco_so() {
        let s = transmit(7, b"abc", 3, 2);
        assert!(s.starts_with("\x1b_Ga=T,f=100,t=d,i=7,U=1,c=3,r=2,q=2,m=0;YWJj"), "{s:?}");
        assert!(s.ends_with("\x1b\\"));
        assert_eq!(s.matches("\x1b_G").count(), 1);
    }

    #[test]
    fn transmit_grande_divide_em_pedacos_de_4096() {
        let s = transmit(1, &vec![7u8; 4000], 2, 2); // base64 de 4000 bytes = 5336 caracteres
        assert_eq!(s.matches("\x1b_G").count(), 2);
        assert!(s.contains("m=1;") && s.contains("\x1b_Gq=2,m=0;"), "{s:?}");
    }

    #[test]
    fn png_codificado_comeca_com_a_assinatura() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 255])));
        let png = encode_png(&img).unwrap();
        assert_eq!(&png[..4], b"\x89PNG");
    }

    #[test]
    fn deteccao_de_terminal() {
        assert!(supports_kitty(env(&[("TERM", "xterm-kitty")])));
        assert!(supports_kitty(env(&[("KITTY_WINDOW_ID", "3")])));
        assert!(supports_kitty(env(&[("TERM_PROGRAM", "ghostty")])));
        assert!(!supports_kitty(env(&[("TERM", "xterm-256color")])));
        assert!(!supports_kitty(env(&[("TERM", "xterm-kitty"), ("TMUX", "/tmp/x")])));
        assert!(!supports_kitty(env(&[])));
    }
}
