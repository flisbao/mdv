//! Copiar para a área de transferência pelo terminal (OSC 52), sem `xclip`/`pbcopy`.

/// Maior texto enviado de uma vez; terminais costumam recusar payloads maiores.
pub const MAX_COPY_BYTES: usize = 64 * 1024;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 padrão (RFC 4648) com `=`.
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(chunk[0]) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        for i in 0..4 {
            if i <= chunk.len() {
                let idx = (n >> (18 - 6 * i)) & 63;
                out.push(char::from(ALPHABET[idx as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodifica base64 padrão (RFC 4648). Espaço em branco é ignorado e o `=` final é opcional.
/// `None` se houver caractere fora do alfabeto, dado depois do `=`, padding excessivo (> 2 `=`),
/// padding sem dados ou tamanho impossível.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut padding = 0usize;
    for b in s.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None;
        }
        let v = ALPHABET.iter().position(|&a| a == b)?;
        acc = (acc << 6) | u32::try_from(v).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).ok()?);
        }
    }
    if padding > 2 || bits >= 6 {
        return None;
    }
    if padding > 0 && out.is_empty() && bits == 0 {
        return None;
    }
    Some(out)
}

/// Sequência OSC 52 que copia `text`. `Err(n)` se o texto (em bytes) passa de `MAX_COPY_BYTES`.
pub fn osc52(text: &str) -> Result<String, usize> {
    let clean = crate::render::sanitize(text);
    if clean.len() > MAX_COPY_BYTES {
        return Err(clean.len());
    }
    Ok(format!("\x1b]52;c;{}\x07", base64(clean.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_segue_os_vetores_da_rfc_4648() {
        let casos = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (entrada, saida) in casos {
            assert_eq!(base64(entrada.as_bytes()), saida, "entrada {entrada:?}");
        }
    }

    #[test]
    fn base64_aceita_bytes_arbitrarios() {
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64(&[0x00]), "AA==");
    }

    #[test]
    fn base64_decode_inverte_o_encode_e_aceita_sem_padding() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            assert_eq!(base64_decode(&base64(s.as_bytes())).unwrap(), s.as_bytes(), "{s}");
        }
        assert_eq!(base64_decode("Zg").unwrap(), b"f");
        assert_eq!(base64_decode("Zm9v\nYmFy").unwrap(), b"foobar"); // espaço em branco é ignorado
    }

    #[test]
    fn base64_decode_recusa_entrada_invalida() {
        assert!(base64_decode("@@@@").is_none());
        assert!(base64_decode("Z").is_none()); // 6 bits sozinhos
        assert!(base64_decode("Zg==Zg").is_none()); // dado depois do padding
        assert!(base64_decode("AA=====").is_none()); // excesso de padding
        assert!(base64_decode("====").is_none()); // só padding sem dados
    }

    #[test]
    fn osc52_monta_a_sequencia_para_a_area_de_transferencia() {
        assert_eq!(osc52("abc").unwrap(), "\x1b]52;c;YWJj\x07");
    }

    #[test]
    fn osc52_sanitiza_controles_antes_do_base64() {
        assert_eq!(osc52("a\x1bb\x07c").unwrap(), osc52("a\u{FFFD}b\u{FFFD}c").unwrap());
        assert!(!osc52("a\x1b[31mb").unwrap().contains("[31m"), "o payload é só base64");
    }

    #[test]
    fn osc52_recusa_o_que_passa_do_teto_e_informa_o_tamanho() {
        let no_teto = "x".repeat(MAX_COPY_BYTES);
        assert!(osc52(&no_teto).is_ok());
        let grande = "x".repeat(MAX_COPY_BYTES + 1);
        assert_eq!(osc52(&grande), Err(MAX_COPY_BYTES + 1));
    }
}
