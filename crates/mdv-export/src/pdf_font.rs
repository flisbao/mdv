//! Fontes padrão do PDF (Helvetica e Courier): codificação WinAnsi (cp1252) e larguras dos glifos.
//! As larguras de Helvetica valem para as faces normal e oblíqua; as de Helvetica-Bold, para a
//! negrito e a negrito-oblíqua. Letras acentuadas usam a largura da letra base (diferença menor
//! que 6% de um em, só afeta a quebra de linha, nunca o desenho do glifo).

/// Face usada em um trecho de texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
}

impl Face {
    pub fn of(bold: bool, italic: bool, mono: bool) -> Face {
        match (mono, bold, italic) {
            (true, _, _) => Face::Mono,
            (false, true, true) => Face::BoldItalic,
            (false, true, false) => Face::Bold,
            (false, false, true) => Face::Italic,
            (false, false, false) => Face::Regular,
        }
    }

    /// Nome do recurso de fonte na página (`/F1`...).
    pub fn resource(self) -> &'static str {
        match self {
            Face::Regular => "F1",
            Face::Bold => "F2",
            Face::Italic => "F3",
            Face::BoldItalic => "F4",
            Face::Mono => "F5",
        }
    }

    /// Nome PostScript da fonte padrão.
    pub fn base_font(self) -> &'static str {
        match self {
            Face::Regular => "Helvetica",
            Face::Bold => "Helvetica-Bold",
            Face::Italic => "Helvetica-Oblique",
            Face::BoldItalic => "Helvetica-BoldOblique",
            Face::Mono => "Courier",
        }
    }

    pub const ALL: [Face; 5] = [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic, Face::Mono];
}

/// Byte WinAnsi do caractere, ou `None` se a fonte padrão não o tem.
pub fn encode(c: char) -> Option<u8> {
    let cp = c as u32;
    match cp {
        0x20..=0x7E | 0xA0..=0xFF => Some(cp as u8),
        _ => match c {
            '€' => Some(0x80),
            '‚' => Some(0x82),
            'ƒ' => Some(0x83),
            '„' => Some(0x84),
            '…' => Some(0x85),
            '†' => Some(0x86),
            '‡' => Some(0x87),
            'ˆ' => Some(0x88),
            '‰' => Some(0x89),
            'Š' => Some(0x8A),
            '‹' => Some(0x8B),
            'Œ' => Some(0x8C),
            'Ž' => Some(0x8E),
            '‘' => Some(0x91),
            '’' => Some(0x92),
            '“' => Some(0x93),
            '”' => Some(0x94),
            '•' => Some(0x95),
            '–' => Some(0x96),
            '—' => Some(0x97),
            '˜' => Some(0x98),
            '™' => Some(0x99),
            'š' => Some(0x9A),
            '›' => Some(0x9B),
            'œ' => Some(0x9C),
            'ž' => Some(0x9E),
            'Ÿ' => Some(0x9F),
            _ => None,
        },
    }
}

/// Larguras de Helvetica para os códigos 32..=126, em milésimos de em.
const REGULAR: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32-47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 48-63
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // 64-79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // 80-95
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 96-111
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // 112-126
];

/// Larguras de Helvetica-Bold para os códigos 32..=126.
const BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, // 32-47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, // 48-63
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, // 64-79
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, // 80-95
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, // 96-111
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584, // 112-126
];

/// Letra ASCII cuja largura representa o caractere Latin-1 acentuado.
fn base_letter(b: u8) -> u8 {
    match b {
        0xC0..=0xC5 => b'A',
        0xC7 => b'C',
        0xC8..=0xCB => b'E',
        0xCC..=0xCF => b'I',
        0xD0 => b'D',
        0xD1 => b'N',
        0xD2..=0xD6 | 0xD8 => b'O',
        0xD9..=0xDC => b'U',
        0xDD => b'Y',
        0xDE => b'P',
        0xDF => b'b',
        0xE0..=0xE5 => b'a',
        0xE6 => b'm',
        0xE7 => b'c',
        0xE8..=0xEB => b'e',
        0xEC..=0xEF => b'i',
        0xF0 | 0xF2..=0xF6 | 0xF8 => b'o',
        0xF1 => b'n',
        0xF9..=0xFC => b'u',
        0xFD | 0xFF => b'y',
        0xFE => b'p',
        0xD7 | 0xF7 => b'+',
        _ => b'n',
    }
}

/// Largura do byte `b` na face, em milésimos de em.
pub fn width(face: Face, b: u8) -> u32 {
    if face == Face::Mono {
        return 600;
    }
    let table = if matches!(face, Face::Bold | Face::BoldItalic) { &BOLD } else { &REGULAR };
    let ascii = |c: u8| u32::from(table[usize::from(c - 32)]);
    match b {
        32..=126 => ascii(b),
        0x85 | 0x97 => 1000,
        0x91 | 0x92 => 222,
        0x93 | 0x94 => 333,
        0x95 => 350,
        0x96 => 556,
        0xA0 => ascii(b' '),
        0xA1..=0xBF => ascii(b'n'),
        0xC0..=0xFF => ascii(base_letter(b)),
        _ => ascii(b'n'),
    }
}

/// Largura de uma sequência de bytes, em pontos, no tamanho `size`.
pub fn text_width(face: Face, bytes: &[u8], size: f32) -> f32 {
    bytes.iter().map(|&b| width(face, b)).sum::<u32>() as f32 * size / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_acentos_e_aspas_tem_byte() {
        assert_eq!(encode('A'), Some(b'A'));
        assert_eq!(encode('é'), Some(0xE9));
        assert_eq!(encode('ç'), Some(0xE7));
        assert_eq!(encode('“'), Some(0x93));
        assert_eq!(encode('—'), Some(0x97));
        assert_eq!(encode('•'), Some(0x95));
    }

    #[test]
    fn fora_do_winansi_nao_tem_byte() {
        assert_eq!(encode('→'), None);
        assert_eq!(encode('😀'), None);
        assert_eq!(encode('日'), None);
        assert_eq!(encode('\u{1b}'), None);
    }

    #[test]
    fn larguras_conhecidas_de_helvetica() {
        assert_eq!(width(Face::Regular, b' '), 278);
        assert_eq!(width(Face::Regular, b'a'), 556);
        assert_eq!(width(Face::Regular, b'W'), 944);
        assert_eq!(width(Face::Bold, b'a'), 556);
        assert_eq!(width(Face::Bold, b'b'), 611);
        assert_eq!(width(Face::Mono, b'i'), 600);
        assert_eq!(width(Face::Regular, 0xE9), width(Face::Regular, b'e'));
    }

    #[test]
    fn largura_do_texto_em_pontos() {
        // "aa" em 10 pt: 2 × 556 / 1000 × 10 = 11,12.
        assert!((text_width(Face::Regular, b"aa", 10.0) - 11.12).abs() < 0.001);
    }

    #[test]
    fn toda_face_tem_recurso_e_nome() {
        for f in Face::ALL {
            assert!(f.resource().starts_with('F') && !f.base_font().is_empty());
        }
        assert_eq!(Face::of(true, true, false), Face::BoldItalic);
        assert_eq!(Face::of(true, true, true), Face::Mono);
    }
}
