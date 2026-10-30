//! Estilos do `word/styles.xml`: quais são títulos, citação, código, negrito/itálico e numeração.
//! Só interpreta o arquivo; quem usa os estilos fica nos módulos de parágrafo e de run.

use std::collections::HashMap;

use crate::propriedades::ler_ppr;
use crate::xml_leitor::{Evento, Leitor};
use crate::Budget;

/// Limite de saltos em `basedOn`; cadeia mais longa, ou ciclo, para aqui.
const MAX_SALTOS: usize = 16;

/// Estilo como aparece no XML, antes de seguir `basedOn`.
struct Bruto {
    id: String,
    base: Option<String>,
    proprio: EstiloDoc,
}

/// O que um estilo significa para a conversão.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(crate) struct EstiloDoc {
    pub titulo: Option<u8>,
    pub citacao: bool,
    pub codigo: bool,
    pub negrito: bool,
    pub italico: bool,
    pub numeracao: Option<(u32, u8)>,
}

/// Estilos já com a herança de `basedOn` resolvida, indexados pelo `styleId`.
pub(crate) struct Estilos {
    mapa: HashMap<String, EstiloDoc>,
}

impl Estilos {
    /// Sem nenhum estilo: todo `styleId` cai no padrão.
    pub(crate) fn vazio() -> Self {
        Estilos { mapa: HashMap::new() }
    }

    /// Lê `styles.xml`; estilo desconhecido não é erro.
    pub(crate) fn ler(xml: &[u8], budget: &dyn Budget) -> Result<Self, String> {
        let mut leitor = Leitor::novo(xml, budget);
        let mut brutos = Vec::new();
        while let Some(tag) = leitor.ate("style")? {
            let id = tag.attr("styleId").unwrap_or("").to_string();
            brutos.push(ler_estilo(&mut leitor, id)?);
        }
        Ok(Estilos { mapa: herdar(&brutos, budget)? })
    }

    /// Estilo pelo `styleId`; id desconhecido devolve o padrão (sem efeito).
    pub(crate) fn de(&self, id: &str) -> EstiloDoc {
        self.mapa.get(id).copied().unwrap_or_default()
    }
}

/// Lê um `w:style` até o fecho. Só `name`, `basedOn` e `pPr` importam; o resto (`rPr`, `tblPr`...)
/// é pulado, pois as flags de fonte de um estilo saem do nome e não do `rPr`.
fn ler_estilo(leitor: &mut Leitor, id: String) -> Result<Bruto, String> {
    let mut nome = String::new();
    let mut base = None;
    let mut titulo_outline = None;
    let mut numeracao = None;
    loop {
        match leitor.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "style" => break,
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "name" => {
                    nome = tag.val().unwrap_or("").trim().to_lowercase();
                    leitor.pular()?;
                }
                "basedOn" => {
                    base = tag.val().map(str::to_string);
                    leitor.pular()?;
                }
                // `numId` 0 quer dizer "sem numeração" e `outlineLvl` 9, texto comum.
                "pPr" => {
                    let props = ler_ppr(leitor)?;
                    titulo_outline = props.nivel;
                    numeracao = props.num.filter(|&(num_id, _)| num_id != 0);
                }
                _ => leitor.pular()?,
            },
            Some(_) => {}
        }
    }
    // O nome tem prioridade sobre `outlineLvl`: o Word usa o nível do outline só como reserva.
    let mut proprio = efeito_nome(&nome);
    proprio.titulo = proprio.titulo.or(titulo_outline);
    proprio.numeracao = numeracao;
    Ok(Bruto { id, base, proprio })
}

/// Resolve `basedOn` de cada estilo. Campos `Option` vêm do primeiro da cadeia que os define
/// (o próprio estilo vence o pai); flags `bool` são o OU da cadeia. A cadeia para em
/// `MAX_SALTOS`, então um ciclo (A em B, B em A) não trava a leitura.
fn herdar(brutos: &[Bruto], budget: &dyn Budget) -> Result<HashMap<String, EstiloDoc>, String> {
    let indice: HashMap<&str, usize> =
        brutos.iter().enumerate().map(|(i, b)| (b.id.as_str(), i)).collect();
    let mut mapa = HashMap::with_capacity(brutos.len());
    for bruto in brutos {
        budget.check()?;
        let mut acumulado = EstiloDoc::default();
        let mut atual = Some(bruto);
        for _ in 0..MAX_SALTOS {
            let Some(b) = atual else { break };
            acumulado = juntar(acumulado, b.proprio);
            atual = b
                .base
                .as_deref()
                .and_then(|id| indice.get(id))
                .map(|&i| &brutos[i]);
        }
        mapa.insert(bruto.id.clone(), acumulado);
    }
    Ok(mapa)
}

/// Junta o estilo já acumulado (filho) com um ancestral: `Option` do filho primeiro, flags em OU.
fn juntar(filho: EstiloDoc, pai: EstiloDoc) -> EstiloDoc {
    EstiloDoc {
        titulo: filho.titulo.or(pai.titulo),
        citacao: filho.citacao || pai.citacao,
        codigo: filho.codigo || pai.codigo,
        negrito: filho.negrito || pai.negrito,
        italico: filho.italico || pai.italico,
        numeracao: filho.numeracao.or(pai.numeracao),
    }
}

/// Efeito de um estilo pelo nome em minúsculas. Os estilos embutidos mantêm o nome em inglês
/// mesmo no Word em outro idioma, e o `styleId` varia; por isso o casamento é pelo nome.
fn efeito_nome(nome: &str) -> EstiloDoc {
    let titulo = match nome {
        "title" => Some(1),
        _ => nome
            .strip_prefix("heading ")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=9).contains(n))
            .map(|n| n.min(6)),
    };
    EstiloDoc {
        titulo,
        citacao: matches!(nome, "quote" | "intense quote" | "block text"),
        codigo: matches!(
            nome,
            "code" | "source code" | "preformatted text" | "html preformatted" | "html code"
                | "verbatim char"
        ),
        negrito: nome == "strong",
        italico: nome == "emphasis",
        numeracao: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Unlimited;

    /// Monta `w:styles` com estes filhos e lê.
    fn ler_xml(corpo: &str) -> Estilos {
        let xml = format!("<w:styles>{corpo}</w:styles>");
        Estilos::ler(xml.as_bytes(), &Unlimited).unwrap()
    }

    #[test]
    fn titulo_pelo_nome_mesmo_com_id_estranho() {
        let e = ler_xml(r#"<w:style w:styleId="Ttulo1"><w:name w:val="heading 1"/></w:style>"#);
        assert_eq!(e.de("Ttulo1").titulo, Some(1));
    }

    #[test]
    fn titulo_acima_de_6_vira_6() {
        let e = ler_xml(r#"<w:style w:styleId="H9"><w:name w:val="heading 9"/></w:style>"#);
        assert_eq!(e.de("H9").titulo, Some(6));
    }

    #[test]
    fn title_vira_nivel_1() {
        let e = ler_xml(r#"<w:style w:styleId="Titulo"><w:name w:val="Title"/></w:style>"#);
        assert_eq!(e.de("Titulo").titulo, Some(1));
    }

    #[test]
    fn heranca_por_based_on() {
        let e = ler_xml(
            r#"<w:style w:styleId="Heading2"><w:name w:val="heading 2"/></w:style>
               <w:style w:styleId="Meu"><w:basedOn w:val="Heading2"/></w:style>"#,
        );
        assert_eq!(e.de("Meu").titulo, Some(2));
    }

    #[test]
    fn ciclo_de_based_on_nao_trava() {
        let e = ler_xml(
            r#"<w:style w:styleId="A"><w:basedOn w:val="B"/></w:style>
               <w:style w:styleId="B"><w:basedOn w:val="A"/></w:style>"#,
        );
        assert_eq!(e.de("A").titulo, None);
        assert_eq!(e.de("B").titulo, None);
    }

    #[test]
    fn outline_lvl_do_estilo() {
        let e = ler_xml(
            r#"<w:style w:styleId="Dois"><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
               <w:style w:styleId="Corpo"><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>"#,
        );
        assert_eq!(e.de("Dois").titulo, Some(3));
        assert_eq!(e.de("Corpo").titulo, None);
    }

    #[test]
    fn flags_por_nome() {
        let e = ler_xml(
            r#"<w:style w:styleId="Q"><w:name w:val="Quote"/></w:style>
               <w:style w:styleId="C"><w:name w:val="Source Code"/></w:style>
               <w:style w:styleId="S"><w:name w:val="Strong"/></w:style>
               <w:style w:styleId="E"><w:name w:val="Emphasis"/></w:style>"#,
        );
        assert!(e.de("Q").citacao);
        assert!(e.de("C").codigo);
        assert!(e.de("S").negrito);
        assert!(e.de("E").italico);
    }

    #[test]
    fn numeracao_no_estilo() {
        let e = ler_xml(
            r#"<w:style w:styleId="L1"><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="3"/></w:numPr></w:pPr></w:style>
               <w:style w:styleId="L0"><w:pPr><w:numPr><w:numId w:val="3"/></w:numPr></w:pPr></w:style>
               <w:style w:styleId="Nada"><w:pPr><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr></w:style>"#,
        );
        assert_eq!(e.de("L1").numeracao, Some((3, 1)));
        assert_eq!(e.de("L0").numeracao, Some((3, 0)));
        assert_eq!(e.de("Nada").numeracao, None);
    }

    #[test]
    fn id_desconhecido_da_padrao() {
        let e = ler_xml(r#"<w:style w:styleId="X"><w:name w:val="heading 1"/></w:style>"#);
        assert_eq!(e.de("inexistente"), EstiloDoc::default());
    }
}
