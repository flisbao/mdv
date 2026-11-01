//! Montador de ZIP para testes; só existe em testes (ou com a feature `testutil`).

use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;

/// Monta um ZIP mínimo com as entradas dadas; `deflate` comprime as não vazias.
pub fn zip(entradas: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    montar(entradas, deflate, &[])
}

/// Monta um ZIP sem compressão com `comentario` no fim (EOCD), para testar a busca do diretório.
pub fn zip_com_comentario(entradas: &[(&str, &[u8])], comentario: &[u8]) -> Vec<u8> {
    montar(entradas, false, comentario)
}

/// Monta o ZIP inteiro: cabeçalhos locais, diretório central e EOCD com `comentario`.
fn montar(entradas: &[(&str, &[u8])], deflate: bool, comentario: &[u8]) -> Vec<u8> {
    let mut saida = Vec::new();
    let mut central = Vec::new();
    for (nome, dados) in entradas {
        let offset = saida.len() as u32;
        let (metodo, corpo) = if deflate && !dados.is_empty() {
            (8u16, comprimir(dados))
        } else {
            (0u16, dados.to_vec())
        };
        let (tam_comp, tam_orig) = (corpo.len() as u32, dados.len() as u32);
        saida.extend_from_slice(b"PK\x03\x04");
        for x in [20u16, 0, metodo, 0, 0] {
            saida.extend_from_slice(&x.to_le_bytes());
        }
        saida.extend_from_slice(&0u32.to_le_bytes());
        saida.extend_from_slice(&tam_comp.to_le_bytes());
        saida.extend_from_slice(&tam_orig.to_le_bytes());
        saida.extend_from_slice(&(nome.len() as u16).to_le_bytes());
        saida.extend_from_slice(&0u16.to_le_bytes());
        saida.extend_from_slice(nome.as_bytes());
        saida.extend_from_slice(&corpo);

        central.extend_from_slice(b"PK\x01\x02");
        for x in [20u16, 20, 0, metodo, 0, 0] {
            central.extend_from_slice(&x.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&tam_comp.to_le_bytes());
        central.extend_from_slice(&tam_orig.to_le_bytes());
        central.extend_from_slice(&(nome.len() as u16).to_le_bytes());
        for x in [0u16, 0, 0, 0] {
            central.extend_from_slice(&x.to_le_bytes());
        }
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(nome.as_bytes());
    }
    let inicio_central = saida.len() as u32;
    let tam_central = central.len() as u32;
    let total = entradas.len() as u16;
    saida.extend_from_slice(&central);
    saida.extend_from_slice(b"PK\x05\x06");
    for x in [0u16, 0, total, total] {
        saida.extend_from_slice(&x.to_le_bytes());
    }
    saida.extend_from_slice(&tam_central.to_le_bytes());
    saida.extend_from_slice(&inicio_central.to_le_bytes());
    saida.extend_from_slice(&(comentario.len() as u16).to_le_bytes());
    saida.extend_from_slice(comentario);
    saida
}

/// Deflate cru (sem cabeçalho zlib), como o ZIP exige para o método 8.
fn comprimir(dados: &[u8]) -> Vec<u8> {
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(dados).expect("deflate em memória");
    enc.finish().expect("deflate em memória")
}

#[cfg(test)]
mod tests {
    use super::zip;

    #[test]
    fn zip_comeca_com_assinatura_local() {
        let bytes = zip(&[("a", "x".as_bytes())], false);
        assert!(bytes.starts_with(b"PK\x03\x04"));
    }
}
