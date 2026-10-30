//! Leitura de um parágrafo (`w:p`): reúne os runs, resolve links e entrega ao classificador.
//! Revisões, controles de conteúdo e marcas de compatibilidade são tratados aqui.

use crate::classificar::classificar;
use crate::contexto::Contexto;
use crate::modelo::{Bloco, Trecho};
use crate::propriedades::{ler_ppr, Propriedades};
use crate::trecho::ler_run;
use crate::xml_leitor::{Evento, Leitor};

/// Lê um `w:p` (chamada logo depois do `Abre("p")`), consome até o `Fecha` dele e o classifica.
/// Quase todo elemento é transparente (`ins`, `sdt`, `smartTag`, `Choice`, `bookmarkStart`...):
/// não se faz nada e o laço desce até os runs dentro dele. Só são pulados o que não é texto
/// aceito: `del` e `moveFrom` (texto apagado), `Fallback` (cópia do `Choice`), `drawing`/`pict`
/// e `txbxContent` (caixa de texto, fora do escopo) e `rPr` solto.
pub(crate) fn ler_paragrafo(l: &mut Leitor, ctx: &mut Contexto) -> Result<Option<Bloco>, String> {
    let mut props = Propriedades::default();
    let mut trechos = Vec::new();
    percorrer(l, ctx, None, &mut props, &mut trechos, "p")?;
    Ok(classificar(&props, trechos, ctx))
}

/// Percorre os eventos até o `Fecha(fim)`. Dentro de um `hyperlink` volta aqui com o destino,
/// que vale para todo run até o fecho dele, mesmo atrás de `ins` ou `smartTag`.
fn percorrer(
    l: &mut Leitor,
    ctx: &mut Contexto,
    link: Option<&str>,
    props: &mut Propriedades,
    trechos: &mut Vec<Trecho>,
    fim: &str,
) -> Result<(), String> {
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == fim => return Ok(()),
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "pPr" => *props = ler_ppr(l)?,
                "r" => ler_run(l, ctx, link, trechos)?,
                "hyperlink" => {
                    // Só destino externo vira link; `anchor` aponta para dentro do documento.
                    let url = tag
                        .attr("id")
                        .and_then(|id| ctx.relacoes.por_id(id))
                        .filter(|r| r.externo)
                        .map(|r| r.alvo.clone());
                    percorrer(l, ctx, url.as_deref(), props, trechos, "hyperlink")?;
                }
                "del" | "moveFrom" | "Fallback" | "drawing" | "pict" | "txbxContent" | "rPr" => {
                    l.pular()?
                }
                _ => {}
            },
            Some(_) => {}
        }
    }
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
