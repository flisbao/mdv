//! Leitura de inteiros little-endian com verificação de limites.

/// Lê `n` bytes a partir de `pos`; `pos + n` que estoura o `usize` vira erro, não pânico.
pub(crate) fn fatia(b: &[u8], pos: usize, n: usize) -> Result<&[u8], String> {
    pos.checked_add(n)
        .and_then(|fim| b.get(pos..fim))
        .ok_or_else(|| "ZIP truncado".to_string())
}

/// Lê um `u16` little-endian na posição `pos`.
pub(crate) fn u16_em(b: &[u8], pos: usize) -> Result<u16, String> {
    let s = fatia(b, pos, 2)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

/// Lê um `u32` little-endian na posição `pos`.
pub(crate) fn u32_em(b: &[u8], pos: usize) -> Result<u32, String> {
    let s = fatia(b, pos, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[cfg(test)]
mod tests {
    use super::{u16_em, u32_em};

    #[test]
    fn le_little_endian() {
        assert_eq!(u16_em(&[1, 2], 0), Ok(0x0201));
        assert_eq!(u32_em(&[1, 2, 3, 4], 0), Ok(0x04030201));
    }

    #[test]
    fn faltando_byte_da_erro() {
        assert!(u32_em(&[1, 2, 3], 0).is_err());
    }

    #[test]
    fn posicao_enorme_nao_entra_em_panico() {
        assert!(u16_em(&[1, 2], usize::MAX).is_err());
    }
}
