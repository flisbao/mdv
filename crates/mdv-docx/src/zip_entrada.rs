//! Extração de uma entrada do ZIP: confere o tamanho declarado e descomprime com teto.

use crate::le::u16_em;
use crate::zip_diretorio::Entrada;
use crate::MAX_PARTE_BYTES;
use flate2::read::DeflateDecoder;
use std::io::Read;

/// Assinatura do cabeçalho local de cada entrada.
const LOCAL_SIG: &[u8] = b"PK\x03\x04";
/// Tamanho fixo do cabeçalho local, sem nome e extra.
const LOCAL_TAM: usize = 30;

/// Extrai e descomprime uma entrada; o tamanho declarado é conferido antes de qualquer leitura.
/// O `take(tam_original + 1)` limita a memória mesmo com deflate de bomba: nunca passa de um byte
/// além do declarado, e esse byte extra só serve para acusar a inconsistência.
/// Os tamanhos do cabeçalho local não são confiados (com o bit 3 de flags eles vêm zerados).
pub(crate) fn extrair(bytes: &[u8], e: &Entrada) -> Result<Vec<u8>, String> {
    if e.tam_original as usize > MAX_PARTE_BYTES {
        return Err(format!("parte do .docx grande demais: {}", e.nome));
    }
    let dados = localizar(bytes, e)?;
    let saida = match e.metodo {
        0 => {
            // Confere antes de copiar: o tamanho declarado já passou do teto, mas os bytes
            // guardados vêm do arquivo e podem ser bem maiores que ele.
            if dados.len() != e.tam_original as usize {
                return Err(format!("ZIP inválido: tamanho inconsistente em {}", e.nome));
            }
            dados.to_vec()
        }
        8 => {
            let mut saida = Vec::with_capacity(e.tam_original as usize);
            DeflateDecoder::new(dados)
                .take(e.tam_original as u64 + 1)
                .read_to_end(&mut saida)
                .map_err(|_| format!("ZIP inválido: deflate corrompido em {}", e.nome))?;
            saida
        }
        m => return Err(format!("método de compressão não suportado: {m}")),
    };
    if saida.len() != e.tam_original as usize {
        return Err(format!("ZIP inválido: tamanho inconsistente em {}", e.nome));
    }
    Ok(saida)
}

/// Localiza os bytes comprimidos da entrada a partir do cabeçalho local, com somas verificadas.
fn localizar<'a>(bytes: &'a [u8], e: &Entrada) -> Result<&'a [u8], String> {
    let fora = || "ZIP inválido: entrada fora do arquivo".to_string();
    let pos = e.posicao as usize;
    let fim_cab = pos.checked_add(LOCAL_TAM).ok_or_else(fora)?;
    let cab = bytes.get(pos..fim_cab).ok_or_else(fora)?;
    if &cab[..4] != LOCAL_SIG {
        return Err("ZIP inválido: cabeçalho local ausente".into());
    }
    let nome = u16_em(cab, 26)? as usize;
    let extra = u16_em(cab, 28)? as usize;
    let inicio = pos.checked_add(LOCAL_TAM + nome + extra).ok_or_else(fora)?;
    let fim = inicio.checked_add(e.tam_comprimido as usize).ok_or_else(fora)?;
    bytes.get(inicio..fim).ok_or_else(fora)
}

#[cfg(test)]
mod tests {
    use super::extrair;
    use crate::testutil::zip;
    use crate::zip_diretorio::{listar, Entrada};
    use crate::MAX_PARTE_BYTES;

    /// Primeira entrada do ZIP montado, já listada.
    fn primeira(bytes: &[u8]) -> Entrada {
        listar(bytes).expect("lista").remove(0)
    }

    #[test]
    fn extrai_armazenada() {
        let bytes = zip(&[("x", b"ola")], false);
        assert_eq!(extrair(&bytes, &primeira(&bytes)), Ok(b"ola".to_vec()));
    }

    #[test]
    fn extrai_deflate() {
        let original = "abc".repeat(1000);
        let bytes = zip(&[("x", original.as_bytes())], true);
        assert_eq!(extrair(&bytes, &primeira(&bytes)), Ok(original.into_bytes()));
    }

    #[test]
    fn tamanho_declarado_acima_do_teto_e_recusado_sem_alocar() {
        let e = Entrada {
            nome: "x".into(),
            metodo: 8,
            tam_comprimido: 10,
            tam_original: (MAX_PARTE_BYTES as u32) + 1,
            posicao: 0,
        };
        let erro = extrair(&[], &e).unwrap_err();
        assert!(erro.contains("grande demais"), "{erro}");
    }

    #[test]
    fn declarado_menor_que_o_real_e_recusado() {
        // Bomba: 1 MiB de zeros comprimidos, declarados como 100 bytes.
        let zeros = vec![0u8; 1 << 20];
        let bytes = zip(&[("x", zeros.as_slice())], true);
        let mut e = primeira(&bytes);
        e.tam_original = 100;
        let erro = extrair(&bytes, &e).unwrap_err();
        assert!(erro.contains("tamanho inconsistente"), "{erro}");
    }

    #[test]
    fn armazenada_com_tamanho_diferente_e_recusada_sem_copiar() {
        let bytes = zip(&[("x", b"0123456789")], false);
        let mut e = primeira(&bytes);
        e.tam_original = 4;
        let erro = extrair(&bytes, &e).unwrap_err();
        assert!(erro.contains("tamanho inconsistente"), "{erro}");
    }

    #[test]
    fn declarado_maior_que_o_real_e_recusado() {
        let bytes = zip(&[("x", b"0123456789")], true);
        let mut e = primeira(&bytes);
        e.tam_original = 15;
        let erro = extrair(&bytes, &e).unwrap_err();
        assert!(erro.contains("tamanho inconsistente"), "{erro}");
    }

    #[test]
    fn metodo_desconhecido_e_recusado() {
        let bytes = zip(&[("x", b"oi")], false);
        let mut e = primeira(&bytes);
        e.metodo = 99;
        let erro = extrair(&bytes, &e).unwrap_err();
        assert!(erro.contains("método"), "{erro}");
    }

    #[test]
    fn posicao_fora_do_arquivo_da_erro() {
        let bytes = zip(&[("x", b"oi")], false);
        let mut e = primeira(&bytes);
        e.posicao = u32::MAX;
        assert!(extrair(&bytes, &e).is_err());
    }
}
