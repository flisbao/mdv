//! Relações OPC (`.rels`): liga o `Id` de cada relação ao tipo, ao alvo e a se ela é externa.
//! Também resolve os alvos relativos ao caminho da parte dona das relações.

use std::collections::HashMap;

use crate::xml_leitor::Leitor;
use crate::Budget;

/// Uma relação do pacote; `tipo` é só o último segmento da URL (ex.: `styles`, `hyperlink`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Relacao {
    pub tipo: String,
    pub alvo: String,
    pub externo: bool,
}

/// Relações de uma parte, na ordem em que aparecem no arquivo `.rels`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Relacoes {
    itens: Vec<(String, Relacao)>,
    /// `Id` → posição em `itens`, para `por_id` não varrer a lista a cada `r:id` do documento.
    indice: HashMap<String, usize>,
}

impl Relacoes {
    /// Conjunto sem relações, usado quando a parte não tem arquivo `.rels`.
    pub(crate) fn vazias() -> Self {
        Self::default()
    }

    /// Lê um `.rels`; relação sem `Id`, `Type` ou `Target` é ignorada.
    pub(crate) fn ler(xml: &[u8], budget: &dyn Budget) -> Result<Self, String> {
        let mut leitor = Leitor::novo(xml, budget);
        let mut itens = Vec::new();
        let mut indice = HashMap::new();
        while let Some(tag) = leitor.ate("Relationship")? {
            let (Some(id), Some(tipo), Some(alvo)) = (tag.attr("Id"), tag.attr("Type"), tag.attr("Target"))
            else {
                continue;
            };
            // `TargetMode` pode vir em qualquer caixa; o Word grava `External`.
            let externo = tag.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
            let tipo = tipo.rsplit_once('/').map_or(tipo, |(_, ultimo)| ultimo).to_string();
            let relacao = Relacao { tipo, alvo: alvo.to_string(), externo };
            // `Id` repetido: vale o primeiro, o mesmo que a varredura em ordem daria.
            indice.entry(id.to_string()).or_insert(itens.len());
            itens.push((id.to_string(), relacao));
        }
        Ok(Self { itens, indice })
    }

    /// Relação pelo `Id` (ex.: `rId5`).
    pub(crate) fn por_id(&self, id: &str) -> Option<&Relacao> {
        self.indice.get(id).map(|&i| &self.itens[i].1)
    }

    /// Primeira relação cujo tipo (último segmento da URL) é `tipo`.
    pub(crate) fn por_tipo(&self, tipo: &str) -> Option<&Relacao> {
        self.itens.iter().map(|(_, r)| r).find(|r| r.tipo == tipo)
    }
}

/// Caminho de `alvo` dentro do pacote, relativo à parte `base` (que é a parte dona das relações).
/// Alvo com `/` inicial é absoluto e ignora a pasta da base; `..` sobe uma pasta, e subir além da
/// raiz é ignorado (o pacote não tem pasta acima da raiz, então o `..` extra só some).
pub(crate) fn resolver(base: &str, alvo: &str) -> String {
    let completo = match alvo.strip_prefix('/') {
        Some(absoluto) => absoluto.to_string(),
        None => {
            let pasta = base.rsplit_once('/').map_or("", |(pasta, _)| pasta);
            format!("{pasta}/{alvo}")
        }
    };
    let mut partes: Vec<&str> = Vec::new();
    for segmento in completo.split('/') {
        match segmento {
            "" | "." => {}
            ".." => {
                partes.pop();
            }
            outro => partes.push(outro),
        }
    }
    partes.join("/")
}

#[cfg(test)]
mod tests {
    use super::{resolver, Relacoes};
    use crate::Unlimited;

    const REL: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://a.org" TargetMode="External"/></Relationships>"#;

    #[test]
    fn le_relacoes() {
        let rels = Relacoes::ler(REL.as_bytes(), &Unlimited).expect("lê");
        let link = rels.por_id("rId2").expect("rId2");
        assert!(link.externo);
        assert_eq!(link.tipo, "hyperlink");
        assert_eq!(rels.por_tipo("styles").expect("styles").alvo, "styles.xml");
        assert!(!rels.por_id("rId1").expect("rId1").externo);
        assert!(rels.por_id("rId9").is_none());
    }

    #[test]
    fn id_repetido_devolve_o_primeiro() {
        let xml = r#"<Relationships><Relationship Id="rId1" Type="x/hyperlink" Target="primeiro" TargetMode="External"/><Relationship Id="rId1" Type="x/hyperlink" Target="segundo"/></Relationships>"#;
        let rels = Relacoes::ler(xml.as_bytes(), &Unlimited).expect("lê");
        let r = rels.por_id("rId1").expect("rId1");
        assert_eq!(r.alvo, "primeiro");
        assert!(r.externo);
    }

    #[test]
    fn resolver_caminhos() {
        assert_eq!(resolver("word/document.xml", "styles.xml"), "word/styles.xml");
        assert_eq!(resolver("word/document.xml", "../customXml/a.xml"), "customXml/a.xml");
        assert_eq!(resolver("word/document.xml", "/word/numbering.xml"), "word/numbering.xml");
        assert_eq!(resolver("word/document.xml", "../../../x"), "x");
    }
}
