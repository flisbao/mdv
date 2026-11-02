//! Arquivos `.docx` do Word: leitura com teto e conversão para Markdown (crate `mdv-docx`).
//! O texto devolvido já é Markdown, então o resto do `mdv` (TUI, `--plain`, `-o .md`) o trata
//! como qualquer documento.

use std::fs;
use std::io;
use std::path::Path;

use mdv_docx::{Budget, MAX_DOCX_BYTES};

use crate::{load, mem};

/// Extensão `.docx` (sem diferenciar caixa).
pub(crate) fn is_docx(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("docx"))
}

/// O limite de memória do processo (`mem`) visto pelo `mdv-docx`.
struct DocxBudget;

impl Budget for DocxBudget {
    fn check(&self) -> Result<(), String> {
        mem::check().map_err(|e| e.to_string())
    }
}

/// Lê um `.docx` regular, recusando o que passa de `MAX_DOCX_BYTES` antes de ler, e devolve o
/// Markdown do documento. Erro de conversão vira `io::Error` com a mensagem em português.
pub(crate) fn read_docx_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    if meta.len() > MAX_DOCX_BYTES {
        let teto = MAX_DOCX_BYTES / (1024 * 1024);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("arquivo .docx grande demais (máximo {teto} MiB)"),
        ));
    }
    mem::check_input(meta.len()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let bytes = load::read_limited_bytes(fs::File::open(path)?, MAX_DOCX_BYTES)?;
    mdv_docx::to_markdown(&bytes, &DocxBudget).map_err(io::Error::other)
}

/// Documento que o `mdv` não lê (`.doc` antigo, `.docm`, `.dotx`, `.dotm`, `.odt`): a mensagem em
/// português que manda salvar como `.docx`. `None` para os outros arquivos.
pub(crate) fn documento_nao_suportado(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "doc" => "documento .doc (formato antigo) não é suportado: salve como .docx",
        "docm" => "documento .docm não é suportado ainda: salve como .docx",
        "dotx" => "modelo .dotx não é suportado ainda: salve como .docx",
        "dotm" => "modelo .dotm não é suportado ainda: salve como .docx",
        "odt" => "documento .odt não é suportado ainda: salve como .docx",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdv_docx::testutil::DocxBuilder;
    use std::fs::File;

    #[test]
    fn extensao_docx_sem_caixa() {
        assert!(is_docx(Path::new("A.DOCX")));
        assert!(is_docx(Path::new("pasta/a.docx")));
        assert!(!is_docx(Path::new("a.md")));
        assert!(!is_docx(Path::new("docx")));
        assert!(!is_docx(Path::new("a.docx.md")));
    }

    #[test]
    fn documentos_nao_suportados_dao_erro_claro_antes_do_markdown() {
        let d = tempfile::tempdir().unwrap();
        for nome in ["a.doc", "a.DOCM", "a.dotx", "a.dotm", "a.odt"] {
            let p = d.path().join(nome);
            fs::write(&p, b"qualquer coisa").unwrap();
            let err = crate::load::read_document(&p).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{nome}");
            assert!(err.to_string().contains("salve como .docx"), "{nome}: {err}");
            assert!(!err.to_string().contains("UTF-8"), "{nome}: {err}");
        }
        assert!(documento_nao_suportado(Path::new("a.doc")).unwrap().contains(".doc (formato antigo)"));
        assert!(documento_nao_suportado(Path::new("a.docx")).is_none());
        assert!(documento_nao_suportado(Path::new("a.md")).is_none());
        assert!(documento_nao_suportado(Path::new("semextensao")).is_none());
    }

    #[test]
    fn le_docx_do_disco() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("oi.docx");
        fs::write(&p, DocxBuilder::novo().corpo("<w:p><w:r><w:t>Oi</w:t></w:r></w:p>").construir()).unwrap();
        let md = read_docx_file(&p).unwrap();
        assert!(md.contains("Oi"), "{md}");
    }

    #[test]
    fn docx_acima_do_teto_e_recusado_sem_ler_tudo() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grande.docx");
        File::create(&p).unwrap().set_len(MAX_DOCX_BYTES + 1).unwrap();
        let err = read_docx_file(&p).unwrap_err();
        assert!(err.to_string().contains("grande demais"), "{err}");
    }

    #[test]
    fn diretorio_nao_e_arquivo_regular() {
        let d = tempfile::tempdir().unwrap();
        let err = read_docx_file(d.path()).unwrap_err();
        assert!(err.to_string().contains("não é um arquivo regular"), "{err}");
    }
}
