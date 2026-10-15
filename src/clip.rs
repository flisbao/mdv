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
