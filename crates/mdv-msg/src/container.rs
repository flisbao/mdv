//! Acesso ao contêiner OLE2 (MS-CFB) e às propriedades do MS-OXMSG: streams com teto de tamanho,
//! storages numerados (destinatários e anexos) e a tabela de propriedades fixas.

use crate::codepage;
use crate::MAX_MSG_BYTES;
use cfb::CompoundFile;
use std::io::{Cursor, Read};

/// Contêiner aberto sobre bytes em memória.
pub(crate) type Cfb<'a> = CompoundFile<Cursor<&'a [u8]>>;

pub(crate) const ERRO_NAO_OLE: &str = "não é um arquivo .msg válido (contêiner OLE2 ausente)";
pub(crate) const ERRO_GRANDE: &str = "anexo/propriedade grande demais no .msg";
pub(crate) const ERRO_ILEGIVEL: &str = "arquivo .msg corrompido (contêiner ilegível)";

/// Tipos de propriedade (parte baixa da tag ou sufixo do nome do stream).
pub(crate) const TIPO_LONG: u16 = 0x0003;
pub(crate) const TIPO_BOOLEANO: u16 = 0x000B;
pub(crate) const TIPO_OBJETO: u16 = 0x000D;
pub(crate) const TIPO_STRING8: u16 = 0x001E;
pub(crate) const TIPO_UNICODE: u16 = 0x001F;
pub(crate) const TIPO_TEMPO: u16 = 0x0040;
pub(crate) const TIPO_BINARIO: u16 = 0x0102;

/// Nome do stream de propriedades fixas em cada storage.
pub(crate) const PROPRIEDADES: &str = "__properties_version1.0";

/// Ids de propriedade (MS-OXPROPS) usados pelo leitor e pelo montador de testes.
pub mod tags {
    pub const SUBJECT: u16 = 0x0037;
    pub const CLIENT_SUBMIT_TIME: u16 = 0x0039;
    pub const MESSAGE_DELIVERY_TIME: u16 = 0x0E06;
    pub const MESSAGE_CLASS: u16 = 0x001A;
    pub const TRANSPORT_MESSAGE_HEADERS: u16 = 0x007D;
    pub const SENDER_NAME: u16 = 0x0C1A;
    pub const SENDER_EMAIL_ADDRESS: u16 = 0x0C1F;
    pub const SENDER_SMTP_ADDRESS: u16 = 0x5D01;
    pub const SENT_REPRESENTING_NAME: u16 = 0x0042;
    pub const SENT_REPRESENTING_SMTP_ADDRESS: u16 = 0x5D02;
    pub const DISPLAY_TO: u16 = 0x0E04;
    pub const DISPLAY_CC: u16 = 0x0E03;
    pub const BODY: u16 = 0x1000;
    pub const RTF_COMPRESSED: u16 = 0x1009;
    pub const HTML: u16 = 0x1013;
    pub const MESSAGE_CODEPAGE: u16 = 0x3FFD;
    pub const INTERNET_CPID: u16 = 0x3FDE;
    pub const RECIPIENT_TYPE: u16 = 0x0C15;
    pub const DISPLAY_NAME: u16 = 0x3001;
    pub const EMAIL_ADDRESS: u16 = 0x3003;
    pub const SMTP_ADDRESS: u16 = 0x39FE;
    pub const ATTACH_LONG_FILENAME: u16 = 0x3707;
    pub const ATTACH_FILENAME: u16 = 0x3704;
    pub const ATTACH_SIZE: u16 = 0x0E20;
    pub const ATTACH_METHOD: u16 = 0x3705;
    pub const ATTACH_MIME_TAG: u16 = 0x370E;
    pub const ATTACH_DATA: u16 = 0x3701;
    pub const ATTACHMENT_HIDDEN: u16 = 0x7FFE;
}

/// Junta um diretório (`/` ou vazio para a raiz) e um nome filho.
pub(crate) fn junta(dir: &str, nome: &str) -> String {
    if dir.is_empty() || dir == "/" {
        format!("/{nome}")
    } else {
        format!("{dir}/{nome}")
    }
}

/// Nome do stream de uma propriedade variável: `__substg1.0_{ID}{TIPO}` em hexadecimal maiúsculo.
pub(crate) fn nome_propriedade(id: u16, tipo: u16) -> String {
    format!("__substg1.0_{id:04X}{tipo:04X}")
}

/// Abre o contêiner; bytes que não são OLE2 viram erro claro.
pub(crate) fn abre(bytes: &[u8]) -> Result<Cfb<'_>, String> {
    CompoundFile::open(Cursor::new(bytes)).map_err(|_| ERRO_NAO_OLE.to_string())
}

/// Lê um stream inteiro, recusando os que passam de `MAX_MSG_BYTES`. Ausente vira `None`.
pub(crate) fn le_stream(cfb: &mut Cfb<'_>, caminho: &str) -> Result<Option<Vec<u8>>, String> {
    if !cfb.is_stream(caminho) {
        return Ok(None);
    }
    let tamanho = cfb.entry(caminho).map_err(|_| ERRO_ILEGIVEL.to_string())?.len();
    if tamanho > MAX_MSG_BYTES {
        return Err(ERRO_GRANDE.to_string());
    }
    let mut fluxo = cfb.open_stream(caminho).map_err(|_| ERRO_ILEGIVEL.to_string())?;
    let mut dados = Vec::new();
    fluxo
        .by_ref()
        .take(MAX_MSG_BYTES + 1)
        .read_to_end(&mut dados)
        .map_err(|_| ERRO_ILEGIVEL.to_string())?;
    if dados.len() as u64 > MAX_MSG_BYTES {
        return Err(ERRO_GRANDE.to_string());
    }
    Ok(Some(dados))
}

/// Storages filhos de `dir` cujo nome é `prefixo` + número hexadecimal, em ordem numérica.
/// Devolve no máximo `max` caminhos e um sinal de que houve mais.
pub(crate) fn filhos_numerados(
    cfb: &Cfb<'_>,
    dir: &str,
    prefixo: &str,
    max: usize,
) -> Result<(Vec<String>, bool), String> {
    let entradas = cfb.read_storage(dir).map_err(|_| ERRO_ILEGIVEL.to_string())?;
    let mut achados: Vec<(u32, String)> = Vec::new();
    for entrada in entradas {
        let nome = entrada.name();
        let Some(hex) = nome.strip_prefix(prefixo) else {
            continue;
        };
        let Ok(numero) = u32::from_str_radix(hex, 16) else {
            continue;
        };
        if entrada.is_storage() {
            achados.push((numero, junta(dir, nome)));
        }
    }
    achados.sort_by_key(|(numero, _)| *numero);
    let truncado = achados.len() > max;
    achados.truncate(max);
    Ok((achados.into_iter().map(|(_, caminho)| caminho).collect(), truncado))
}

/// Tabela de propriedades fixas de um storage: cabeçalho e entradas de 16 bytes.
pub(crate) struct Propriedades {
    bytes: Vec<u8>,
    cabecalho: usize,
}

impl Propriedades {
    /// Lê `__properties_version1.0` do storage; ausente equivale a nenhuma propriedade.
    pub(crate) fn ler(cfb: &mut Cfb<'_>, dir: &str, cabecalho: usize) -> Result<Self, String> {
        let bytes = le_stream(cfb, &junta(dir, PROPRIEDADES))?.unwrap_or_default();
        Ok(Self { bytes, cabecalho })
    }

    /// Valor bruto (8 bytes) da propriedade com este id e tipo.
    pub(crate) fn valor(&self, id: u16, tipo: u16) -> Option<[u8; 8]> {
        let alvo = (u32::from(id) << 16) | u32::from(tipo);
        let (entradas, _) = self.bytes.get(self.cabecalho..)?.as_chunks::<16>();
        entradas
            .iter()
            .find(|e| u32::from_le_bytes([e[0], e[1], e[2], e[3]]) == alvo)
            .map(|e| [e[8], e[9], e[10], e[11], e[12], e[13], e[14], e[15]])
    }

    pub(crate) fn inteiro(&self, id: u16) -> Option<i32> {
        self.valor(id, TIPO_LONG)
            .map(|v| i32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }

    pub(crate) fn booleano(&self, id: u16) -> Option<bool> {
        self.valor(id, TIPO_BOOLEANO).map(|v| v[0] != 0)
    }

    pub(crate) fn tempo(&self, id: u16) -> Option<u64> {
        self.valor(id, TIPO_TEMPO).map(u64::from_le_bytes)
    }
}

/// Decodifica UTF-16LE (unidades inválidas viram U+FFFD).
pub(crate) fn decodifica_utf16le(bytes: &[u8]) -> String {
    let (pares, _) = bytes.as_chunks::<2>();
    let unidades: Vec<u16> = pares.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16_lossy(&unidades)
}

/// Texto sanitizado e sem NULs finais; vazio vira `None`.
fn limpa(bruto: &str) -> Option<String> {
    let aparado = bruto.trim_end_matches('\0');
    if aparado.is_empty() {
        None
    } else {
        Some(mdv_text::sanitize(aparado).into_owned())
    }
}

/// Texto da propriedade: tenta UTF-16LE (`001F`) e depois bytes na codepage (`001E`).
pub(crate) fn texto(cfb: &mut Cfb<'_>, dir: &str, id: u16, cp: u32) -> Result<Option<String>, String> {
    if let Some(bytes) = le_stream(cfb, &junta(dir, &nome_propriedade(id, TIPO_UNICODE)))? {
        return Ok(limpa(&decodifica_utf16le(&bytes)));
    }
    match le_stream(cfb, &junta(dir, &nome_propriedade(id, TIPO_STRING8)))? {
        Some(bytes) => Ok(limpa(&codepage::decode(&bytes, cp))),
        None => Ok(None),
    }
}

/// Só o texto UTF-16LE (`001F`) da propriedade.
pub(crate) fn texto_unicode(cfb: &mut Cfb<'_>, dir: &str, id: u16) -> Result<Option<String>, String> {
    let bytes = le_stream(cfb, &junta(dir, &nome_propriedade(id, TIPO_UNICODE)))?;
    Ok(bytes.and_then(|b| limpa(&decodifica_utf16le(&b))))
}

/// Bytes brutos de uma propriedade do tipo indicado.
pub(crate) fn binario(cfb: &mut Cfb<'_>, dir: &str, id: u16, tipo: u16) -> Result<Option<Vec<u8>>, String> {
    le_stream(cfb, &junta(dir, &nome_propriedade(id, tipo)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nome_de_propriedade_em_hexadecimal_maiusculo() {
        assert_eq!(nome_propriedade(0x3701, TIPO_BINARIO), "__substg1.0_37010102");
        assert_eq!(nome_propriedade(0x0037, TIPO_UNICODE), "__substg1.0_0037001F");
    }

    #[test]
    fn caminhos_de_storage_na_raiz_e_aninhados() {
        assert_eq!(junta("/", "a"), "/a");
        assert_eq!(junta("", "a"), "/a");
        assert_eq!(junta("/x", "a"), "/x/a");
    }

    #[test]
    fn utf16_com_nuls_finais_e_limpo() {
        assert_eq!(decodifica_utf16le(&[0x41, 0, 0x42, 0, 0, 0]), "AB\0");
        assert_eq!(limpa("AB\0\0"), Some("AB".to_string()));
        assert_eq!(limpa("\0\0"), None);
    }
}
