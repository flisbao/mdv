//! Leitura de arquivos XML: teto de bytes antes de ler e decodificação pelo `encoding` declarado.

use std::fs;
use std::io;
use std::path::Path;

use crate::data::MAX_DATA_BYTES;
use crate::{load, mem};

/// Quantos bytes do início são procurados em busca de `encoding=` na declaração `<?xml ?>`.
const SNIFF_BYTES: usize = 200;

/// Extensão de arquivo XML (`.xml`, sem diferenciar caixa).
pub(crate) fn is_xml(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xml"))
}

/// Lê um arquivo XML regular, recusando o que passa de `MAX_DATA_BYTES` antes de ler, e decodifica
/// o texto (BOM, `encoding=` declarado ou UTF-8).
pub(crate) fn read_xml_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    let teto = MAX_DATA_BYTES as u64;
    if meta.len() > teto {
        let msg = format!("arquivo XML grande demais (máximo {} MiB)", teto / (1024 * 1024));
        return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
    }
    mem::check_input(meta.len()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let bytes = load::read_limited_bytes(fs::File::open(path)?, teto)?;
    Ok(decode(&bytes))
}

/// Lê o XML de stdin (até `MAX_DATA_BYTES`) e decodifica como `read_xml_file`.
pub(crate) fn read_xml_stdin() -> io::Result<String> {
    read_xml(io::stdin().lock())
}

fn read_xml(reader: impl io::Read) -> io::Result<String> {
    Ok(decode(&load::read_limited_bytes(reader, MAX_DATA_BYTES as u64)?))
}

/// Texto do XML: BOM (UTF-8/UTF-16) manda; senão o `encoding=` da declaração, se for um rótulo
/// conhecido (UTF-16 sem BOM e as codificações `replacement` são ignorados); senão UTF-8 com substituição de bytes inválidos.
pub(crate) fn decode(bytes: &[u8]) -> String {
    if let Some((enc, _)) = encoding_rs::Encoding::for_bom(bytes) {
        return enc.decode(bytes).0.into_owned();
    }
    let enc = encoding_declarado(bytes)
        .and_then(|rotulo| encoding_rs::Encoding::for_label(rotulo.as_bytes()))
        // `replacement` (hz-gb-2312, iso-2022-kr…) devolveria só U+FFFD: melhor ler como UTF-8.
        .filter(|enc| !matches!(enc.name(), "UTF-16LE" | "UTF-16BE" | "replacement"));
    match enc {
        Some(enc) => enc.decode(bytes).0.into_owned(),
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// O rótulo de `encoding="…"` na declaração `<?xml …?>` do início (ASCII, aspas simples ou duplas,
/// sem diferenciar caixa).
fn encoding_declarado(bytes: &[u8]) -> Option<String> {
    const CHAVE: &[u8] = b"encoding";
    let cabeca = bytes.get(..SNIFF_BYTES).unwrap_or(bytes).to_ascii_lowercase();
    let fim_decl = cabeca.windows(2).position(|w| w == b"?>").unwrap_or(cabeca.len());
    let decl = cabeca.get(..fim_decl)?;
    if !decl.starts_with(b"<?xml") {
        return None;
    }
    let pos = decl.windows(CHAVE.len()).position(|w| w == CHAVE)?;
    let depois = decl.get(pos + CHAVE.len()..)?;
    let depois = depois.get(depois.iter().position(|b| !b.is_ascii_whitespace())?..)?;
    let depois = depois.strip_prefix(b"=")?;
    let depois = depois.get(depois.iter().position(|b| !b.is_ascii_whitespace())?..)?;
    let aspas = *depois.first().filter(|&&q| q == b'"' || q == b'\'')?;
    let resto = depois.get(1..)?;
    let rotulo = resto.get(..resto.iter().position(|&b| b == aspas)?)?;
    String::from_utf8(rotulo.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn extensoes() {
        for a in ["a.xml", "A.XML", "pasta/x.Xml"] {
            assert!(is_xml(Path::new(a)), "{a}");
        }
        for a in ["a.xsd", "a.yaml", "a.xml.bak", "xml"] {
            assert!(!is_xml(Path::new(a)), "{a}");
        }
    }

    #[test]
    fn decode_iso_8859_1_declarado() {
        let t = decode(b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>caf\xe9</a>");
        assert!(t.contains("café"), "{t}");
    }

    #[test]
    fn decode_windows_1252_declarado() {
        let t = decode(b"<?xml version='1.0' encoding='windows-1252'?><a>\x93oi\x94</a>");
        assert!(t.contains('\u{201c}'), "{t}");
    }

    #[test]
    fn decode_utf8_com_bom() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("<a>ç</a>".as_bytes());
        assert!(decode(&b).contains('ç'));
    }

    #[test]
    fn decode_utf16_le_com_bom() {
        let b: Vec<u8> = [0xFFu8, 0xFE].into_iter().chain("<a>ç</a>".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert!(decode(&b).contains("<a>ç</a>"));
    }

    #[test]
    fn decode_rotulo_desconhecido_cai_em_utf8() {
        let t = decode("<?xml version=\"1.0\" encoding=\"xyz\"?><a>ç</a>".as_bytes());
        assert!(t.contains('ç'), "{t}");
    }

    #[test]
    fn decode_codificacao_replacement_cai_em_utf8() {
        for rotulo in ["hz-gb-2312", "iso-2022-kr"] {
            let xml = format!("<?xml version=\"1.0\" encoding=\"{rotulo}\"?><a>ç</a>");
            let t = decode(xml.as_bytes());
            assert!(t.contains("<a>ç</a>"), "{rotulo}: {t}");
        }
    }

    #[test]
    fn decode_sem_declaracao_e_utf8_com_substituicao() {
        let t = decode(b"<a>x\xffy</a>");
        assert!(t.contains('\u{fffd}'), "{t}");
    }

    #[test]
    fn ler_leitor_decodifica_e_respeita_o_teto() {
        let t = read_xml(&b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>caf\xe9</a>"[..]).unwrap();
        assert!(t.contains("café"), "{t}");
        let err = read_xml(io::repeat(b'a')).unwrap_err();
        assert!(err.to_string().contains("entrada maior que 5 MiB"), "{err}");
    }

    #[test]
    fn ler_arquivo_decodifica() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.xml");
        fs::write(&p, b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>caf\xe9</a>").unwrap();
        let t = read_xml_file(&p).unwrap();
        assert!(t.contains("café"), "{t}");
    }

    #[test]
    fn arquivo_acima_do_teto_e_recusado_sem_ler() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grande.xml");
        File::create(&p).unwrap().set_len(MAX_DATA_BYTES as u64 + 1).unwrap();
        let err = read_xml_file(&p).unwrap_err();
        assert!(err.to_string().contains("grande demais"), "{err}");
    }

    #[test]
    fn nao_le_diretorio() {
        let d = tempfile::tempdir().unwrap();
        let err = read_xml_file(d.path()).unwrap_err();
        assert!(err.to_string().contains("não é um arquivo regular"), "{err}");
    }
}
