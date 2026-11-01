//! Referência de célula (`A1`, `AB12`) → índice de coluna em base 0.
//! Só as letras iniciais contam: o número da linha nunca define nada na memória.

/// Coluna (base 0) das letras iniciais de `referencia`; sem letras, `None`.
/// Letras demais saturam em vez de estourar; uma coluna enorme cai fora do teto de colunas.
pub(crate) fn coluna_de(referencia: &str) -> Option<usize> {
    let mut coluna: usize = 0;
    let mut letras = 0usize;
    for c in referencia.bytes().take_while(u8::is_ascii_alphabetic) {
        let valor = (c.to_ascii_uppercase() - b'A') as usize + 1;
        coluna = coluna.saturating_mul(26).saturating_add(valor);
        letras += 1;
    }
    (letras > 0).then(|| coluna - 1)
}
