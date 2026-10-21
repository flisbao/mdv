//! Descompressão do corpo RTF de `.msg` (MS-OXRTFCP): formatos `LZFu` e `MELA`.

const INIT_DICT: &[u8] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

const DICT_SIZE: usize = 4096;
const CABECALHO: usize = 16;
const ERRO_TRUNCADO: &str = "corpo RTF corrompido (cabeçalho truncado)";
const ERRO_CORROMPIDO: &str = "corpo RTF corrompido";

/// Tabela CRC-32 refletida (polinômio 0xEDB88320), gerada em tempo de compilação.
const CRC_TABLE: [u32; 256] = gera_tabela_crc();

const fn gera_tabela_crc() -> [u32; 256] {
    let mut tabela = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        tabela[i] = c;
        i += 1;
    }
    tabela
}

/// CRC-32 com valor inicial 0 e sem XOR final, como exige o MS-OXRTFCP.
fn crc32(dados: &[u8]) -> u32 {
    dados.iter().fold(0u32, |crc, &b| {
        CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8)
    })
}

fn u32_le(dados: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([dados[pos], dados[pos + 1], dados[pos + 2], dados[pos + 3]])
}

/// Descomprime um bloco `COMPRESSED_RTF` de `.msg`. `max` limita o `RAWSIZE` aceito, antes de alocar.
pub(crate) fn decompress(data: &[u8], max: usize) -> Result<Vec<u8>, String> {
    if data.len() < CABECALHO {
        return Err(ERRO_TRUNCADO.into());
    }
    let compsize = u32_le(data, 0) as usize;
    let rawsize = u32_le(data, 4) as usize;
    let crc = u32_le(data, 12);
    let fim = match compsize.checked_add(4) {
        Some(f) if f <= data.len() => f,
        _ => return Err(ERRO_TRUNCADO.into()),
    };
    if rawsize > max {
        return Err("corpo RTF grande demais".into());
    }
    let corpo = &data[CABECALHO..fim];
    match &data[8..12] {
        b"MELA" => {
            if rawsize > corpo.len() {
                return Err(ERRO_CORROMPIDO.into());
            }
            Ok(corpo[..rawsize].to_vec())
        }
        b"LZFu" => {
            if crc32(corpo) != crc {
                return Err("corpo RTF corrompido (CRC)".into());
            }
            descomprime_lzfu(corpo, rawsize)
        }
        _ => Err("corpo RTF em formato desconhecido".into()),
    }
}

/// Laço LZFu: bytes de controle com 8 flags (bit 0 = literal, bit 1 = referência de 2 bytes).
fn descomprime_lzfu(corpo: &[u8], rawsize: usize) -> Result<Vec<u8>, String> {
    let mut saida = Vec::with_capacity(rawsize);
    let mut dicionario = [0u8; DICT_SIZE];
    dicionario[..INIT_DICT.len()].copy_from_slice(INIT_DICT);
    let mut escrita = INIT_DICT.len();
    let mut i = 0;
    loop {
        let controle = *corpo.get(i).ok_or(ERRO_CORROMPIDO)?;
        i += 1;
        for bit in 0..8 {
            if controle & (1 << bit) == 0 {
                let b = *corpo.get(i).ok_or(ERRO_CORROMPIDO)?;
                i += 1;
                if saida.len() >= rawsize {
                    return Err(ERRO_CORROMPIDO.into());
                }
                saida.push(b);
                dicionario[escrita] = b;
                escrita = (escrita + 1) % DICT_SIZE;
            } else {
                let par = corpo.get(i..i + 2).ok_or(ERRO_CORROMPIDO)?;
                i += 2;
                let valor = u16::from_be_bytes([par[0], par[1]]) as usize;
                let mut offset = valor >> 4;
                let tamanho = (valor & 0xF) + 2;
                if offset == escrita {
                    return Ok(saida);
                }
                for _ in 0..tamanho {
                    if saida.len() >= rawsize {
                        return Err(ERRO_CORROMPIDO.into());
                    }
                    let b = dicionario[offset];
                    offset = (offset + 1) % DICT_SIZE;
                    saida.push(b);
                    dicionario[escrita] = b;
                    escrita = (escrita + 1) % DICT_SIZE;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    const EXEMPLO: &str = "2d 00 00 00 2b 00 00 00 4c 5a 46 75 f1 c5 c7 a7 03 00 0a 00 72 63 70 67 31 32 35 42 32 0a f3 20 68 65 6c 09 00 20 62 77 05 b0 6c 64 7d 0a 80 0f a0";

    #[test]
    fn dicionario_inicial_tem_207_bytes() {
        assert_eq!(INIT_DICT.len(), 207);
    }

    #[test]
    fn descomprime_exemplo_simples_da_spec() {
        let c = hex(EXEMPLO);
        assert_eq!(
            decompress(&c, 1 << 20).unwrap(),
            b"{\\rtf1\\ansi\\ansicpg1252\\pard hello world}\r\n"
        );
    }

    #[test]
    fn descomprime_referencia_que_sobrepoe_a_escrita() {
        let c = hex("1a 00 00 00 1c 00 00 00 4c 5a 46 75 e2 d4 4b 51 41 00 04 20 57 58 59 5a 0d 6e 7d 01 0e b0");
        assert_eq!(
            decompress(&c, 1 << 20).unwrap(),
            b"{\\rtf1 WXYZWXYZWXYZWXYZWXYZ}"
        );
    }

    #[test]
    fn mela_devolve_os_bytes_sem_compressao() {
        let c = hex("17 00 00 00 0b 00 00 00 4d 45 4c 41 00 00 00 00 7b 5c 72 74 66 31 20 61 62 63 7d");
        assert_eq!(decompress(&c, 1 << 20).unwrap(), b"{\\rtf1 abc}");
    }

    #[test]
    fn crc_errado_e_recusado() {
        let mut c = hex(EXEMPLO);
        c[12] ^= 0xFF;
        let erro = decompress(&c, 1 << 20).unwrap_err();
        assert!(erro.contains("CRC"), "erro inesperado: {erro}");
    }

    #[test]
    fn rawsize_acima_do_teto_e_recusado_sem_alocar() {
        let erro = decompress(&hex(EXEMPLO), 10).unwrap_err();
        assert!(erro.contains("grande demais"), "erro inesperado: {erro}");
    }

    #[test]
    fn entrada_truncada_da_erro() {
        let c = hex(EXEMPLO);
        assert!(decompress(&c[..c.len() - 5], 1 << 20).is_err());
    }

    #[test]
    fn tipo_desconhecido_da_erro() {
        let mut c = hex(EXEMPLO);
        c[8..12].copy_from_slice(b"XXXX");
        let erro = decompress(&c, 1 << 20).unwrap_err();
        assert!(erro.contains("formato desconhecido"), "erro inesperado: {erro}");
    }

    #[test]
    fn qualquer_prefixo_nao_entra_em_panico() {
        let c = hex(EXEMPLO);
        for n in 0..c.len() {
            let _ = decompress(&c[..n], 1 << 20);
        }
    }
}
