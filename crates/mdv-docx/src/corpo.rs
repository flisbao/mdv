//! Leitura do corpo do documento (`word/document.xml`): parágrafos e tabelas na ordem do texto.

use crate::contexto::Contexto;
use crate::modelo::Bloco;
use crate::paragrafo::ler_paragrafo;
use crate::tabela::ler_tabela;
use crate::xml_leitor::{Evento, Leitor};

/// Lê os blocos do corpo na ordem em que aparecem.
/// Um laço plano basta: `p` e `tbl` só aninham dentro das próprias funções que os leem, então tudo
/// o que sobra (`document`, `body`, `sdt`, `sdtContent`...) é transparente e o laço desce por ele.
/// `sectPr` (página e margens) não tem texto e é pulado.
pub(crate) fn ler_corpo(xml: &[u8], ctx: &mut Contexto) -> Result<Vec<Bloco>, String> {
    let mut l = Leitor::novo(xml, ctx.budget);
    let mut blocos = Vec::new();
    while let Some(evento) = l.proximo()? {
        let Evento::Abre(tag) = evento else { continue };
        match tag.nome.as_str() {
            "p" => blocos.extend(ler_paragrafo(&mut l, ctx)?),
            "tbl" => blocos.push(Bloco::Tabela(ler_tabela(&mut l, ctx)?)),
            // `Fallback` é a cópia do `Choice` para leitores antigos: lê-lo duplicaria o bloco.
            "sectPr" | "Fallback" => l.pular()?,
            _ => {}
        }
    }
    Ok(blocos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexto::com_contexto;
    use crate::modelo::{Marcas, Trecho};
    use crate::testutil::NS;

    fn ler(corpo: &str) -> Vec<Bloco> {
        let xml = format!("<w:document {NS}><w:body>{corpo}</w:body></w:document>");
        com_contexto("", "", |ctx| ler_corpo(xml.as_bytes(), ctx).expect("corpo"))
    }

    fn texto(s: &str) -> Trecho {
        Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }
    }

    fn p(s: &str) -> String {
        format!("<w:p><w:r><w:t>{s}</w:t></w:r></w:p>")
    }

    #[test]
    fn mistura_paragrafos_e_tabelas_na_ordem() {
        let tabela = format!("<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", p("celula"));
        let blocos = ler(&format!("{}{tabela}{}", p("antes"), p("depois")));
        assert_eq!(blocos.len(), 3);
        assert_eq!(blocos[0], Bloco::Paragrafo(vec![texto("antes")]));
        assert!(matches!(&blocos[1], Bloco::Tabela(t) if t.linhas.len() == 1));
        assert_eq!(blocos[2], Bloco::Paragrafo(vec![texto("depois")]));
    }

    #[test]
    fn sectpr_e_ignorado() {
        let blocos = ler(&format!(
            "{}<w:sectPr><w:pgSz w:w=\"1\"/><w:p><w:r><w:t>escondido</w:t></w:r></w:p></w:sectPr>",
            p("a")
        ));
        assert_eq!(blocos, vec![Bloco::Paragrafo(vec![texto("a")])]);
    }

    #[test]
    fn sdt_de_bloco_e_transparente() {
        let blocos = ler(&format!("<w:sdt><w:sdtPr/><w:sdtContent>{}</w:sdtContent></w:sdt>", p("dentro")));
        assert_eq!(blocos, vec![Bloco::Paragrafo(vec![texto("dentro")])]);
    }

    #[test]
    fn paragrafos_vazios_somem() {
        let blocos = ler(&format!("<w:p/>{}<w:p><w:pPr/></w:p>", p("x")));
        assert_eq!(blocos, vec![Bloco::Paragrafo(vec![texto("x")])]);
    }
}
