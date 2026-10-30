//! Pacote `.docx` aberto: lê as partes pelo nome e soma o total descomprimido contra o teto.

use crate::zip_diretorio::{listar, Entrada};
use crate::zip_entrada::extrair;
use crate::MAX_TOTAL_BYTES;

/// Um `.docx` aberto para leitura de partes; empresta os bytes do chamador.
pub(crate) struct Pacote<'a> {
    bytes: &'a [u8],
    entradas: Vec<Entrada>,
    usado: usize,
    teto_total: usize,
}

impl<'a> Pacote<'a> {
    /// Abre o pacote listando o diretório central; não extrai nenhuma parte ainda.
    pub(crate) fn abrir(bytes: &'a [u8]) -> Result<Self, String> {
        Ok(Self {
            bytes,
            entradas: listar(bytes)?,
            usado: 0,
            teto_total: MAX_TOTAL_BYTES,
        })
    }

    /// Diz se existe uma parte com este nome (sem diferenciar caixa).
    pub(crate) fn tem(&self, nome: &str) -> bool {
        self.entradas.iter().any(|e| e.nome.eq_ignore_ascii_case(nome))
    }

    /// Extrai a parte pelo nome; `None` se ela não existe. O teto total vale para a soma de todas
    /// as partes lidas, inclusive a mesma parte pedida de novo: cada leitura conta contra o limite.
    pub(crate) fn parte(&mut self, nome: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(e) = self.entradas.iter().find(|e| e.nome.eq_ignore_ascii_case(nome)) else {
            return Ok(None);
        };
        let dados = extrair(self.bytes, e)?;
        let total = self.usado.saturating_add(dados.len());
        if total > self.teto_total {
            return Err("documento descomprimido grande demais".into());
        }
        self.usado = total;
        Ok(Some(dados))
    }

    /// Troca o teto total; só para testar o limite sem gerar 40 MiB.
    #[cfg(test)]
    pub(crate) fn com_teto_total(mut self, n: usize) -> Self {
        self.teto_total = n;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Pacote;
    use crate::testutil::zip;

    #[test]
    fn parte_devolve_conteudo_e_none_se_nao_existe() {
        let bytes = zip(&[("a.txt", b"oi")], false);
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        assert_eq!(pacote.parte("a.txt"), Ok(Some(b"oi".to_vec())));
        assert_eq!(pacote.parte("nao.txt"), Ok(None));
    }

    #[test]
    fn nome_sem_diferenciar_caixa() {
        let bytes = zip(&[("word/document.xml", b"<x/>")], false);
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        assert!(pacote.tem("WORD/Document.XML"));
        assert_eq!(pacote.parte("WORD/Document.XML"), Ok(Some(b"<x/>".to_vec())));
    }

    #[test]
    fn total_acima_do_teto_e_recusado() {
        let dez = [b'a'; 10];
        let bytes = zip(&[("p1", &dez), ("p2", &dez)], false);
        let mut pacote = Pacote::abrir(&bytes).expect("abre").com_teto_total(15);
        assert!(pacote.parte("p1").expect("primeira cabe").is_some());
        let erro = pacote.parte("p2").unwrap_err();
        assert!(erro.contains("grande demais"), "{erro}");
    }

    #[test]
    fn lixo_vira_erro_de_zip() {
        assert!(Pacote::abrir(b"xx").is_err());
    }
}
