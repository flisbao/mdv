//! Arquivos `.msg` do Outlook: leitura com teto e conversão para Markdown (crate `mdv-msg`).
//! O texto devolvido já é Markdown, então o resto do `mdv` (TUI, `--plain`, `-o .md`) o trata
//! como qualquer documento.

use std::fs;
use std::io;
use std::path::Path;

use mdv_msg::{Budget, MAX_MSG_BYTES};

use crate::{load, mem};

/// Extensão `.msg` (sem diferenciar caixa).
pub(crate) fn is_msg(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("msg"))
}

/// O limite de memória do processo (`mem`) visto pelo `mdv-msg`.
struct MsgBudget;

impl Budget for MsgBudget {
    fn check(&self) -> Result<(), String> {
        mem::check().map_err(|e| e.to_string())
    }
    fn check_html(&self, len: usize) -> Result<(), String> {
        mem::check_html(len).map_err(|e| e.to_string())
    }
}

/// Lê um `.msg` regular, recusando o que passa de `MAX_MSG_BYTES` antes de ler, e devolve o
/// Markdown do e-mail. Erro de conversão vira `io::Error` com a mensagem em português.
pub(crate) fn read_msg_file(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "não é um arquivo regular"));
    }
    if meta.len() > MAX_MSG_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "arquivo .msg grande demais (máximo 32 MiB)"));
    }
    mem::check_input(meta.len()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let bytes = load::read_limited_bytes(fs::File::open(path)?, MAX_MSG_BYTES)?;
    mdv_msg::to_markdown(&bytes, &MsgBudget).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdv_msg::testutil::{tags, MsgBuilder};
    use std::fs::{self, File};

    #[test]
    fn extensao_msg_sem_caixa() {
        assert!(is_msg(Path::new("A.MSG")));
        assert!(is_msg(Path::new("pasta/a.msg")));
        assert!(!is_msg(Path::new("a.md")));
        assert!(!is_msg(Path::new("msg")));
    }

    #[test]
    fn le_msg_do_disco() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("oi.msg");
        fs::write(&p, MsgBuilder::new().string(tags::SUBJECT, "Oi").build()).unwrap();
        let md = read_msg_file(&p).unwrap();
        assert!(md.contains("# Oi"), "{md}");
    }

    #[test]
    fn msg_acima_do_teto_e_recusado_sem_ler_tudo() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("grande.msg");
        File::create(&p).unwrap().set_len(mdv_msg::MAX_MSG_BYTES + 1).unwrap();
        let err = read_msg_file(&p).unwrap_err();
        assert!(err.to_string().contains("grande demais"), "{err}");
    }
}
