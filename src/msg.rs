//! Arquivos `.msg` do Outlook: leitura com teto e conversão para Markdown (crate `mdv-msg`).
//! O texto devolvido já é Markdown, então o resto do `mdv` (TUI, `--plain`, `-o .md`) o trata
//! como qualquer documento.

use std::io;
use std::path::Path;

use mdv_msg::MAX_MSG_BYTES;

use crate::load;
use crate::mem::MemBudget;

/// Lê um `.msg` regular, recusando o que passa de `MAX_MSG_BYTES` antes de ler, e devolve o
/// Markdown do e-mail. Erro de conversão vira `io::Error` com a mensagem em português.
pub(crate) fn read_msg_file(path: &Path) -> io::Result<String> {
    let bytes = load::read_binary_capped(path, MAX_MSG_BYTES, ".msg")?;
    mdv_msg::to_markdown(&bytes, &MemBudget).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdv_msg::testutil::{tags, MsgBuilder};
    use std::fs::{self, File};

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
