//! Contexto da leitura: estilos, numeração, relações, contador de listas e o orçamento de trabalho.
//! Reúne o que os módulos de parágrafo e de run consultam sem repassar cada peça por parâmetro.

use crate::estilos::Estilos;
use crate::numeracao::{Contador, Numeracao};
use crate::relacoes::Relacoes;
use crate::Budget;

/// Estado compartilhado pela leitura do corpo; as partes ficam emprestadas de quem chama.
pub(crate) struct Contexto<'a> {
    pub estilos: &'a Estilos,
    pub numeracao: &'a Numeracao,
    pub relacoes: &'a Relacoes,
    pub contador: Contador,
    pub budget: &'a dyn Budget,
}

impl<'a> Contexto<'a> {
    /// Contexto novo, sem nenhum item contado ainda.
    pub(crate) fn novo(
        estilos: &'a Estilos,
        numeracao: &'a Numeracao,
        relacoes: &'a Relacoes,
        budget: &'a dyn Budget,
    ) -> Self {
        Contexto { estilos, numeracao, relacoes, contador: Contador::novo(), budget }
    }
}

/// Monta um contexto de teste a partir de filhos XML de `styles` e `numbering` (vazio = sem nada)
/// e entrega ao `f`. Relações fixas: `rId5` é um link `https`, `rId6` um link `javascript:` (ambos externos) e `rId7` uma imagem interna.
#[cfg(test)]
pub(crate) fn com_contexto<T>(
    estilos: &str,
    numeracao: &str,
    f: impl FnOnce(&mut Contexto<'_>) -> T,
) -> T {
    use crate::estilos::Estilos;
    use crate::numeracao::Numeracao;
    use crate::relacoes::Relacoes;
    use crate::testutil::NS;
    use crate::Unlimited;

    let estilos = if estilos.is_empty() {
        Estilos::vazio()
    } else {
        let xml = format!("<w:styles {NS}>{estilos}</w:styles>");
        Estilos::ler(xml.as_bytes(), &Unlimited).expect("estilos de teste")
    };
    let numeracao = if numeracao.is_empty() {
        Numeracao::vazia()
    } else {
        let xml = format!("<w:numbering {NS}>{numeracao}</w:numbering>");
        Numeracao::ler(xml.as_bytes(), &Unlimited).expect("numeração de teste")
    };
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://exemplo.org" TargetMode="External"/>
        <Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="javascript:alert(1)" TargetMode="External"/>
        <Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/a.png"/>
    </Relationships>"#;
    let relacoes = Relacoes::ler(rels.as_bytes(), &Unlimited).expect("relações de teste");
    let mut ctx = Contexto::novo(&estilos, &numeracao, &relacoes, &Unlimited);
    f(&mut ctx)
}
