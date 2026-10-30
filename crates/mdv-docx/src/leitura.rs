//! Leitura do documento inteiro: junta estilos, numeração, corpo e notas em um `Documento`.

use std::collections::HashMap;

use crate::contexto::Contexto;
use crate::corpo::ler_corpo;
use crate::estilos::Estilos;
use crate::modelo::Documento;
use crate::notas::ler_notas;
use crate::numeracao::Numeracao;
use crate::partes::Partes;
use crate::Budget;

/// Converte as partes do pacote em `Documento`.
/// A ordem é estilos, numeração, corpo e notas porque o corpo consulta os dois primeiros e as notas
/// reaproveitam o mesmo contexto. Parte opcional ausente usa o padrão vazio, nunca é erro.
pub(crate) fn ler_documento(partes: &Partes, budget: &dyn Budget) -> Result<Documento, String> {
    let estilos = match &partes.estilos {
        Some(xml) => Estilos::ler(xml, budget)?,
        None => Estilos::vazio(),
    };
    let numeracao = match &partes.numeracao {
        Some(xml) => Numeracao::ler(xml, budget)?,
        None => Numeracao::vazia(),
    };
    let mut ctx = Contexto::novo(&estilos, &numeracao, &partes.relacoes, budget);
    let blocos = ler_corpo(&partes.principal, &mut ctx)?;
    let mut notas = HashMap::new();
    if let Some(xml) = &partes.notas {
        ler_notas(xml, false, &mut ctx, &mut notas)?;
    }
    if let Some(xml) = &partes.fim {
        ler_notas(xml, true, &mut ctx, &mut notas)?;
    }
    Ok(Documento { blocos, notas })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modelo::Bloco;
    use crate::pacote::Pacote;
    use crate::testutil::DocxBuilder;
    use crate::Unlimited;

    fn partes(b: DocxBuilder) -> Partes {
        let bytes = b.construir();
        let mut pacote = Pacote::abrir(&bytes).expect("abre");
        Partes::carregar(&mut pacote, &Unlimited).expect("carrega")
    }

    #[test]
    fn monta_documento_com_blocos_e_notas() {
        let p = partes(
            DocxBuilder::novo()
                .corpo(r#"<w:p><w:r><w:t>texto</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p>"#)
                .notas(r#"<w:footnote w:id="1"><w:p><w:r><w:t>nota</w:t></w:r></w:p></w:footnote>"#)
                .fim(r#"<w:endnote w:id="1"><w:p><w:r><w:t>fim</w:t></w:r></w:p></w:endnote>"#),
        );
        let doc = ler_documento(&p, &Unlimited).expect("documento");
        assert_eq!(doc.blocos.len(), 1);
        assert!(doc.notas.contains_key("n1"));
        assert!(doc.notas.contains_key("e1"));
    }

    #[test]
    fn estilos_e_numeracao_ausentes_funcionam_com_padrao() {
        let p = partes(DocxBuilder::novo().corpo("<w:p><w:r><w:t>oi</w:t></w:r></w:p>"));
        assert!(p.estilos.is_none() && p.numeracao.is_none());
        let doc = ler_documento(&p, &Unlimited).expect("documento");
        assert!(matches!(&doc.blocos[..], [Bloco::Paragrafo(_)]));
        assert!(doc.notas.is_empty());
    }

    #[test]
    fn xml_do_corpo_invalido_da_erro_de_xml() {
        let p = partes(DocxBuilder::novo().corpo("<w:p><w:r></w:p>"));
        let erro = ler_documento(&p, &Unlimited).expect_err("erro");
        assert!(erro.starts_with("XML inválido"), "{erro}");
    }
}
