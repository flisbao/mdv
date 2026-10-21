//! Mapeamento de codepages do Windows (`\ansicpg`, `PR_INTERNET_CPID`) para `encoding_rs`.

use encoding_rs::{
    Encoding, BIG5, EUC_JP, EUC_KR, GB18030, GBK, ISO_2022_JP, ISO_8859_10, ISO_8859_13, ISO_8859_14,
    ISO_8859_15, ISO_8859_16, ISO_8859_2, ISO_8859_3, ISO_8859_4, ISO_8859_5, ISO_8859_6, ISO_8859_7,
    ISO_8859_8, KOI8_R, KOI8_U, SHIFT_JIS, UTF_16LE, UTF_8, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252,
    WINDOWS_1253, WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258, WINDOWS_874,
};

/// Devolve o codificador de uma codepage do Windows; desconhecida cai em Windows-1252.
pub(crate) fn encoding_for(cp: u32) -> &'static Encoding {
    match cp {
        65001 => UTF_8,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1252 => WINDOWS_1252,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        874 => WINDOWS_874,
        932 => SHIFT_JIS,
        936 => GBK,
        54936 => GB18030,
        949 => EUC_KR,
        950 => BIG5,
        20866 => KOI8_R,
        21866 => KOI8_U,
        28591 | 20127 => WINDOWS_1252,
        28592 => ISO_8859_2,
        28593 => ISO_8859_3,
        28594 => ISO_8859_4,
        28595 => ISO_8859_5,
        28596 => ISO_8859_6,
        28597 => ISO_8859_7,
        28598 => ISO_8859_8,
        28600 => ISO_8859_10,
        28603 => ISO_8859_13,
        28604 => ISO_8859_14,
        28605 => ISO_8859_15,
        28599 => WINDOWS_1254,
        28601 => WINDOWS_874,
        28606 => ISO_8859_16,
        50220..=50222 => ISO_2022_JP,
        51932 => EUC_JP,
        1200 => UTF_16LE,
        _ => WINDOWS_1252,
    }
}

/// Decodifica bytes da codepage `cp`; bytes inválidos viram U+FFFD.
/// Sem BOM (exceto UTF-8 e UTF-16LE): um texto 1252 que começa com FF FE continua sendo "ÿþ".
pub(crate) fn decode(bytes: &[u8], cp: u32) -> String {
    let encoding = encoding_for(cp);
    let (texto, _) = if cp == 65001 || cp == 1200 {
        let (texto, _, _) = encoding.decode(bytes);
        (texto, false)
    } else {
        encoding.decode_without_bom_handling(bytes)
    };
    texto.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::SHIFT_JIS;

    #[test]
    fn codepages_conhecidas() {
        assert_eq!(decode(b"\xe7", 1252), "ç");
        assert_eq!(decode(b"\xe7", 999_999), "ç"); // padrão 1252
        assert_eq!(decode("ação".as_bytes(), 65001), "ação");
        assert_eq!(encoding_for(932), SHIFT_JIS);
    }

    #[test]
    fn bom_nao_reinterpreta_texto_1252() {
        assert_eq!(decode(b"\xff\xfeA", 1252), "ÿþA");
        assert_eq!(decode("\u{feff}ação".as_bytes(), 65001), "ação");
    }

    #[test]
    fn codepages_28599_e_28601_mapeadas() {
        use encoding_rs::{WINDOWS_1254, WINDOWS_874};
        assert_eq!(encoding_for(28599), WINDOWS_1254);
        assert_eq!(encoding_for(28601), WINDOWS_874);
    }
}
