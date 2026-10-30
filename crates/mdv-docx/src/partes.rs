//! Localização das partes do `.docx`: acha a parte principal e as partes ligadas a ela
//! (estilos, numeração, notas) seguindo as relações do pacote, não nomes fixos.

use crate::pacote::Pacote;
use crate::relacoes::{resolver, Relacoes};
use crate::Budget;

/// Partes já lidas do pacote; as opcionais ficam `None` quando o documento não as tem.
pub(crate) struct Partes {
    pub principal: Vec<u8>,
    pub relacoes: Relacoes,
    pub estilos: Option<Vec<u8>>,
    pub numeracao: Option<Vec<u8>>,
    pub notas: Option<Vec<u8>>,
    pub fim: Option<Vec<u8>>,
}

impl Partes {
    /// Lê a parte principal e as ligadas a ela; a principal é obrigatória.
    /// A principal vem da relação `officeDocument` do pacote, pois ele declara onde está o documento;
    /// `word/document.xml` é só fallback. Tipos casam pelo último segmento da URL, porque o OOXML
    /// transicional e o estrito usam prefixos de URL diferentes para o mesmo tipo.
    pub(crate) fn carregar(pacote: &mut Pacote<'_>, budget: &dyn Budget) -> Result<Partes, String> {
        let nome_principal = achar_principal(pacote, budget)?;
        let principal = pacote
            .parte(&nome_principal)?
            .ok_or_else(|| format!("o arquivo não tem {nome_principal}"))?;
        let relacoes = match pacote.parte(&rels_de(&nome_principal))? {
            Some(xml) => Relacoes::ler(&xml, budget)?,
            None => Relacoes::vazias(),
        };
        let estilos = ligada(pacote, &relacoes, &nome_principal, "styles")?;
        let numeracao = ligada(pacote, &relacoes, &nome_principal, "numbering")?;
        let notas = ligada(pacote, &relacoes, &nome_principal, "footnotes")?;
        let fim = ligada(pacote, &relacoes, &nome_principal, "endnotes")?;
        Ok(Partes { principal, relacoes, estilos, numeracao, notas, fim })
    }
}

/// Nome da parte principal: alvo da relação `officeDocument` da raiz, ou `word/document.xml`.
fn achar_principal(pacote: &mut Pacote<'_>, budget: &dyn Budget) -> Result<String, String> {
    if let Some(raiz) = pacote.parte("_rels/.rels")? {
        let rels = Relacoes::ler(&raiz, budget)?;
        if let Some(relacao) = rels.por_tipo("officeDocument") {
            return Ok(resolver("", &relacao.alvo));
        }
    }
    Ok("word/document.xml".to_string())
}

/// Caminho do `.rels` de uma parte: a pasta ganha `_rels/` e o nome ganha `.rels`.
fn rels_de(parte: &str) -> String {
    match parte.rsplit_once('/') {
        Some((pasta, arquivo)) => format!("{pasta}/_rels/{arquivo}.rels"),
        None => format!("_rels/{parte}.rels"),
    }
}

/// Lê a parte ligada à principal por um tipo de relação; sem relação desse tipo, `None`.
fn ligada(
    pacote: &mut Pacote<'_>,
    relacoes: &Relacoes,
    base: &str,
    tipo: &str,
) -> Result<Option<Vec<u8>>, String> {
    match relacoes.por_tipo(tipo) {
        Some(relacao) => pacote.parte(&resolver(base, &relacao.alvo)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::Partes;
    use crate::pacote::Pacote;
    use crate::testutil::{zip, DocxBuilder};
    use crate::Unlimited;

    #[test]
    fn acha_principal_pelas_relacoes() {
        let bytes = DocxBuilder::novo().corpo("<w:p/>").construir();
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        let partes = Partes::carregar(&mut pacote, &Unlimited).expect("carrega");
        assert!(partes.principal.windows(b"<w:body>".len()).any(|w| w == b"<w:body>"));
    }

    #[test]
    fn sem_document_xml_da_erro() {
        let bytes = zip(&[("[Content_Types].xml", b"<Types/>")], false);
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        let erro = Partes::carregar(&mut pacote, &Unlimited).err().expect("erro");
        assert!(erro.contains("word/document.xml"), "{erro}");
    }

    #[test]
    fn acha_parte_principal_com_outro_nome() {
        let rels = br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/principal.xml"/></Relationships>"#;
        let bytes = zip(&[("_rels/.rels", rels), ("word/principal.xml", b"<w:body>x</w:body>")], false);
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        let partes = Partes::carregar(&mut pacote, &Unlimited).expect("carrega");
        assert_eq!(partes.principal, b"<w:body>x</w:body>".to_vec());
    }

    #[test]
    fn partes_opcionais_ausentes_sao_none() {
        let bytes = DocxBuilder::novo().corpo("<w:p/>").construir();
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        let partes = Partes::carregar(&mut pacote, &Unlimited).expect("carrega");
        assert!(partes.estilos.is_none());
        assert!(partes.numeracao.is_none());
        assert!(partes.notas.is_none());
        assert!(partes.fim.is_none());
    }

    #[test]
    fn acha_estilos_numeracao_e_notas_via_relacao() {
        let bytes = DocxBuilder::novo()
            .corpo("<w:p/>")
            .estilos("<w:style/>")
            .numeracao("<w:abstractNum/>")
            .notas("<w:footnote/>")
            .fim("<w:endnote/>")
            .construir();
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        let partes = Partes::carregar(&mut pacote, &Unlimited).expect("carrega");
        assert!(partes.estilos.is_some());
        assert!(partes.numeracao.is_some());
        assert!(partes.notas.is_some());
        assert!(partes.fim.is_some());
    }
}
