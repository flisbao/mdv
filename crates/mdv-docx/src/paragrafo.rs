//! Leitura de um parágrafo (`w:p`): reúne os runs, resolve links e entrega ao classificador.
//! Revisões, controles de conteúdo e marcas de compatibilidade são tratados aqui.

use std::rc::Rc;

use crate::classificar::classificar;
use crate::contexto::Contexto;
use crate::modelo::{Bloco, Trecho};
use crate::propriedades::{ler_ppr, Propriedades};
use crate::trecho::ler_run;
use crate::xml_leitor::{Evento, Leitor, Tag};
use crate::MAX_URL_BYTES;

/// Lê um `w:p` (chamada logo depois do `Abre("p")`), consome até o `Fecha` dele e o classifica.
/// Quase todo elemento é transparente (`ins`, `sdt`, `smartTag`, `Choice`, `bookmarkStart`...):
/// não se faz nada e o laço desce até os runs dentro dele. Só são pulados os elementos que não
/// são texto aceito: `del` e `moveFrom` (texto apagado), `Fallback` (cópia do `Choice`), `drawing`/`pict`
/// e `txbxContent` (caixa de texto, fora do escopo) e `rPr` solto.
pub(crate) fn ler_paragrafo(l: &mut Leitor, ctx: &mut Contexto) -> Result<Option<Bloco>, String> {
    let mut paragrafo = Paragrafo::default();
    paragrafo.percorrer(l, ctx, "p")?;
    Ok(classificar(&paragrafo.props, paragrafo.trechos, ctx))
}

/// O que a leitura de um `w:p` junta: as propriedades, os trechos e o link em vigor.
#[derive(Default)]
struct Paragrafo {
    props: Propriedades,
    trechos: Vec<Trecho>,
    /// Destino do `hyperlink` aberto, que vale para todo run até o fecho dele.
    link: Option<Rc<str>>,
}

impl Paragrafo {
    /// Percorre os eventos até o `Fecha(fim)`. Dentro de um `hyperlink` volta aqui com o destino,
    /// que vale para todo run até o fecho dele, mesmo atrás de `ins` ou `smartTag`.
    fn percorrer(&mut self, l: &mut Leitor, ctx: &mut Contexto, fim: &str) -> Result<(), String> {
        while let Some(evento) = l.dentro(fim)? {
            let Evento::Abre(tag) = evento else { continue };
            match tag.nome.as_str() {
                "pPr" => self.props = ler_ppr(l)?,
                "r" => ler_run(l, ctx, self.link.as_ref(), &mut self.trechos)?,
                "hyperlink" => {
                    let anterior = std::mem::replace(&mut self.link, destino(&tag, ctx));
                    self.percorrer(l, ctx, "hyperlink")?;
                    self.link = anterior;
                }
                // `oMath` e `oMathPara` têm runs `m:r`/`m:t` com o mesmo nome local dos de texto.
                "del" | "moveFrom" | "Fallback" | "drawing" | "pict" | "txbxContent" | "rPr" | "oMath"
                | "oMathPara" => l.pular()?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Só destino externo vira link; `anchor` aponta para dentro do documento.
fn destino(hyperlink: &Tag, ctx: &Contexto) -> Option<Rc<str>> {
    hyperlink
        .attr("id")
        .and_then(|id| ctx.relacoes.por_id(id))
        .filter(|r| r.externo && r.alvo.len() <= MAX_URL_BYTES)
        .map(|r| Rc::from(r.alvo.as_str()))
}

/// Lê o primeiro `w:p` do XML e o classifica; ajuda os testes de parágrafo e de classificação.
#[cfg(test)]
pub(crate) fn ler_xml(ctx: &mut Contexto, corpo: &str) -> Option<Bloco> {
    use crate::testutil::NS;
    use crate::Unlimited;
    let xml = format!("<w:p {NS}>{corpo}</w:p>");
    let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
    l.ate("p").unwrap().unwrap();
    let bloco = ler_paragrafo(&mut l, ctx).unwrap();
    assert_eq!(l.proximo().unwrap(), None, "o parágrafo deve ser consumido por inteiro");
    bloco
}
