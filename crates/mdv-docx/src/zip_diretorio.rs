//! Leitura do diretório central do ZIP: lista nomes, métodos e tamanhos sem extrair nada.

use crate::le::{fatia, u16_em, u32_em};
use crate::MAX_ENTRADAS;
use std::collections::HashSet;

/// Assinatura de arquivo OLE2 (`.doc` antigo ou `.docx` criptografado).
const OLE2: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Assinatura do registro de fim do diretório central (EOCD).
const EOCD_SIG: &[u8] = b"PK\x05\x06";
/// Assinatura de cada registro do diretório central.
const CENTRAL_SIG: &[u8] = b"PK\x01\x02";
/// Tamanho fixo do EOCD, sem o comentário.
const EOCD_TAM: usize = 22;
/// Tamanho fixo de um registro do diretório central, sem nome, extra e comentário.
const CENTRAL_TAM: usize = 46;
const INVALIDO: &str = "não é um arquivo .docx válido (ZIP inválido)";

/// Uma entrada do diretório central do ZIP.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entrada {
    /// Caminho dentro do ZIP, com `/` como separador.
    pub nome: String,
    /// Método de compressão (0 = guardado, 8 = deflate).
    pub metodo: u16,
    /// Tamanho comprimido declarado no diretório.
    pub tam_comprimido: u32,
    /// Tamanho original declarado no diretório.
    pub tam_original: u32,
    /// Deslocamento do cabeçalho local da entrada no arquivo.
    pub posicao: u32,
}

/// Lista as entradas do diretório central; recusa o que não é um `.docx` legível.
pub(crate) fn listar(bytes: &[u8]) -> Result<Vec<Entrada>, String> {
    if bytes.starts_with(&OLE2) {
        return Err("arquivo criptografado ou no formato .doc antigo: só .docx é suportado".into());
    }
    let eocd = achar_eocd(bytes).ok_or(INVALIDO)?;
    let total = u16_em(bytes, eocd + 10)?;
    let inicio = u32_em(bytes, eocd + 16)?;
    // 0xFFFF e 0xFFFFFFFF são sentinelas do ZIP64, cujo diretório fica em outro registro;
    // sem suporte a ZIP64, recusa-se o arquivo em vez de ler campos errados.
    if total == 0xFFFF || inicio == 0xFFFF_FFFF {
        return Err("ZIP64 não suportado".into());
    }
    let total = total as usize;
    if total > MAX_ENTRADAS {
        return Err("ZIP com entradas demais".into());
    }
    let mut pos = inicio as usize;
    let mut saida = Vec::with_capacity(total);
    let mut nomes = HashSet::with_capacity(total);
    for _ in 0..total {
        let (entrada, proximo) = ler_entrada(bytes, pos)?;
        pos = proximo;
        // Pasta não tem conteúdo; só os arquivos interessam.
        if entrada.nome.ends_with('/') {
            continue;
        }
        // O Word trata "A.xml" e "a.XML" como o mesmo arquivo; duplicata sem diferenciar caixa é ambígua.
        if !nomes.insert(entrada.nome.to_lowercase()) {
            return Err(format!("ZIP com entrada duplicada: {}", entrada.nome));
        }
        saida.push(entrada);
    }
    Ok(saida)
}

/// Procura o EOCD de trás para frente: um comentário do ZIP pode conter a assinatura,
/// e o EOCD verdadeiro é o último, perto do fim. A busca cobre no máximo o comentário
/// possível (65535 bytes), que é o limite do próprio formato.
fn achar_eocd(bytes: &[u8]) -> Option<usize> {
    let maximo = bytes.len().checked_sub(EOCD_TAM)?;
    let minimo = bytes.len().saturating_sub(EOCD_TAM + 65535);
    (minimo..=maximo).rev().find(|&i| bytes[i..].starts_with(EOCD_SIG))
}

/// Lê um registro do diretório central em `pos`; devolve a entrada e a posição do próximo.
/// O bit 0 das flags marca criptografia: o `.docx` protegido não tem como ser lido aqui.
fn ler_entrada(bytes: &[u8], pos: usize) -> Result<(Entrada, usize), String> {
    if bytes.get(pos..).and_then(|s| s.get(..4)) != Some(CENTRAL_SIG) {
        return Err(INVALIDO.into());
    }
    if u16_em(bytes, pos + 8)? & 1 != 0 {
        return Err("arquivo .docx protegido por senha".into());
    }
    let nome_tam = u16_em(bytes, pos + 28)? as usize;
    let extra_tam = u16_em(bytes, pos + 30)? as usize;
    let coment_tam = u16_em(bytes, pos + 32)? as usize;
    let nome = String::from_utf8_lossy(fatia(bytes, pos + CENTRAL_TAM, nome_tam)?).into_owned();
    let proximo = pos
        .checked_add(CENTRAL_TAM + nome_tam + extra_tam + coment_tam)
        .ok_or_else(|| "ZIP truncado".to_string())?;
    let entrada = Entrada {
        nome,
        metodo: u16_em(bytes, pos + 10)?,
        tam_comprimido: u32_em(bytes, pos + 20)?,
        tam_original: u32_em(bytes, pos + 24)?,
        posicao: u32_em(bytes, pos + 42)?,
    };
    Ok((entrada, proximo))
}

#[cfg(test)]
mod tests {
    use super::listar;
    use crate::testutil::{zip, zip_com_comentario};
    use crate::MAX_ENTRADAS;

    /// Posição da última ocorrência de `assinatura` em `bytes`.
    fn ultima(bytes: &[u8], assinatura: &[u8]) -> usize {
        bytes.windows(assinatura.len()).rposition(|w| w == assinatura).expect("assinatura")
    }

    #[test]
    fn lista_entradas_do_zip_montado() {
        let bytes = zip(&[("a.txt", b"oi"), ("dir/b.xml", b"<x/>")], false);
        let entradas = listar(&bytes).expect("lista");
        let nomes: Vec<&str> = entradas.iter().map(|e| e.nome.as_str()).collect();
        assert_eq!(nomes, ["a.txt", "dir/b.xml"]);
        assert_eq!(entradas[0].metodo, 0);
        assert_eq!(entradas[0].tam_original, 2);
    }

    #[test]
    fn zip_com_comentario_no_fim_ainda_abre() {
        let bytes = zip_com_comentario(&[("a", b"x")], b"comentario longo");
        assert_eq!(listar(&bytes).expect("lista").len(), 1);
    }

    #[test]
    fn lixo_nao_e_zip() {
        let erro = listar(b"nao sou zip").unwrap_err();
        assert!(erro.contains("ZIP inválido"), "{erro}");
    }

    #[test]
    fn ole2_vira_erro_de_doc_antigo() {
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        bytes.resize(bytes.len() + 100, 0);
        let erro = listar(&bytes).unwrap_err();
        assert!(erro.contains("criptografado ou no formato .doc antigo"), "{erro}");
    }

    #[test]
    fn diretorio_truncado_da_erro() {
        let bytes = zip(&[("a", b"x")], false);
        let cortado = &bytes[..bytes.len() - 30];
        assert!(listar(cortado).is_err());
    }

    #[test]
    fn entrada_com_senha_e_recusada() {
        let mut bytes = zip(&[("a", b"x")], false);
        let central = ultima(&bytes, b"PK\x01\x02");
        bytes[central + 8] |= 1;
        let erro = listar(&bytes).unwrap_err();
        assert!(erro.contains("senha"), "{erro}");
    }

    #[test]
    fn nome_duplicado_e_recusado() {
        let bytes = zip(&[("a", b"1"), ("a", b"2")], false);
        let erro = listar(&bytes).unwrap_err();
        assert!(erro.contains("duplicad"), "{erro}");
    }

    #[test]
    fn nome_duplicado_sem_diferenciar_caixa_e_recusado() {
        let bytes = zip(&[("A.xml", b"1"), ("a.XML", b"2")], false);
        let erro = listar(&bytes).unwrap_err();
        assert!(erro.contains("duplicad"), "{erro}");
    }

    #[test]
    fn zip64_e_recusado() {
        let mut bytes = zip(&[("a", b"x")], false);
        let eocd = ultima(&bytes, b"PK\x05\x06");
        bytes[eocd + 10..eocd + 12].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let erro = listar(&bytes).unwrap_err();
        assert!(erro.contains("ZIP64"), "{erro}");
    }

    #[test]
    fn mais_entradas_que_o_teto_e_recusado() {
        let nomes: Vec<String> = (0..=MAX_ENTRADAS).map(|i| format!("f{i}")).collect();
        let entradas: Vec<(&str, &[u8])> = nomes.iter().map(|n| (n.as_str(), &b""[..])).collect();
        let erro = listar(&zip(&entradas, false)).unwrap_err();
        assert!(erro.contains("entradas demais"), "{erro}");
    }

    #[test]
    fn deslocamento_do_diretorio_fora_do_arquivo_da_erro() {
        let mut bytes = zip(&[("a", b"x")], false);
        let eocd = ultima(&bytes, b"PK\x05\x06");
        bytes[eocd + 16..eocd + 20].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        assert!(listar(&bytes).is_err());
    }
}
