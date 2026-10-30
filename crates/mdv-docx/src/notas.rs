//! Leitura das notas de rodapé e de fim (`footnotes.xml`, `endnotes.xml`) para o mapa do documento.

use std::collections::HashMap;

use crate::contexto::Contexto;
use crate::modelo::{chave_nota, Marcas, Trecho};
use crate::paragrafo::ler_paragrafo;
use crate::xml_leitor::{Evento, Leitor};

/// Teto de notas guardadas, para um arquivo hostil não encher o mapa sem fim.
const MAX_NOTAS: usize = 100_000;

/// Lê as notas da parte e as guarda em `saida` sob `chave_nota(fim, id)`.
/// O Word põe em cada parte notas especiais (`separator`, `continuationSeparator`,
/// `continuationNotice`) que só desenham a linha de separação: são puladas, só `normal` ou sem tipo
/// valem. A marca de referência dentro da própria nota é descartada, pois uma nota que cita outra
/// poderia formar um ciclo na saída.
pub(crate) fn ler_notas(
    xml: &[u8],
    fim: bool,
    ctx: &mut Contexto,
    saida: &mut HashMap<String, Vec<Trecho>>,
) -> Result<(), String> {
    let elemento = if fim { "endnote" } else { "footnote" };
    let mut l = Leitor::novo(xml, ctx.budget);
    while let Some(evento) = l.proximo()? {
        let Evento::Abre(tag) = evento else { continue };
        if tag.nome != elemento {
            continue;
        }
        let normal = matches!(tag.attr("type"), None | Some("normal"));
        let Some(id) = tag.attr("id").filter(|_| normal) else {
            l.pular()?;
            continue;
        };
        let trechos = ler_nota(&mut l, ctx, elemento)?;
        if saida.len() >= MAX_NOTAS {
            return Err("notas demais".into());
        }
        saida.insert(chave_nota(fim, id), trechos);
    }
    Ok(())
}

/// Lê os parágrafos de uma nota (depois do `Abre`) e os junta em uma só lista de trechos.
fn ler_nota(l: &mut Leitor, ctx: &mut Contexto, elemento: &str) -> Result<Vec<Trecho>, String> {
    let mut trechos: Vec<Trecho> = Vec::new();
    let mut primeiro = true;
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == elemento => return Ok(trechos),
            Some(Evento::Abre(tag)) if tag.nome == "p" => {
                let Some(bloco) = ler_paragrafo(l, ctx)? else { continue };
                for paragrafo in bloco.paragrafos() {
                    if !primeiro {
                        let espaco = Trecho::Texto { texto: " ".into(), marcas: Marcas::default(), link: None };
                        trechos.push(espaco);
                    }
                    primeiro = false;
                    trechos.extend(paragrafo.into_iter().filter(|t| !matches!(t, Trecho::NotaRef(_))));
                }
            }
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexto::com_contexto;
    use crate::testutil::NS;

    fn ler(corpo: &str, fim: bool) -> HashMap<String, Vec<Trecho>> {
        let raiz = if fim { "endnotes" } else { "footnotes" };
        let xml = format!("<w:{raiz} {NS}>{corpo}</w:{raiz}>");
        com_contexto("", "", |ctx| {
            let mut saida = HashMap::new();
            ler_notas(xml.as_bytes(), fim, ctx, &mut saida).expect("notas");
            saida
        })
    }

    fn texto(s: &str) -> Trecho {
        Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }
    }

    #[test]
    fn le_notas_de_rodape() {
        let mapa = ler(
            r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
            <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>
            <w:footnote w:id="1"><w:p><w:r><w:t>um</w:t></w:r></w:p></w:footnote>
            <w:footnote w:type="normal" w:id="2"><w:p><w:r><w:t>dois</w:t></w:r></w:p></w:footnote>"#,
            false,
        );
        let mut chaves: Vec<_> = mapa.keys().cloned().collect();
        chaves.sort();
        assert_eq!(chaves, ["n1", "n2"]);
        assert_eq!(mapa["n1"], vec![texto("um")]);
        assert_eq!(mapa["n2"], vec![texto("dois")]);
    }

    #[test]
    fn varios_paragrafos_viram_um_texto_com_espaco() {
        let mapa = ler(
            r#"<w:footnote w:id="1"><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:footnote>"#,
            false,
        );
        assert_eq!(mapa["n1"], vec![texto("a"), texto(" "), texto("b")]);
    }

    #[test]
    fn notas_de_fim_usam_prefixo_e() {
        let mapa = ler(
            r#"<w:endnote w:id="1"><w:p><w:r><w:t>fim</w:t></w:r></w:p></w:endnote>
            <w:footnote w:id="9"><w:p><w:r><w:t>ignorada</w:t></w:r></w:p></w:footnote>"#,
            true,
        );
        assert_eq!(mapa.len(), 1);
        assert_eq!(mapa["e1"], vec![texto("fim")]);
    }

    #[test]
    fn a_marca_de_referencia_dentro_da_nota_e_descartada() {
        let mapa = ler(
            r#"<w:footnote w:id="1"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t> texto</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p></w:footnote>"#,
            false,
        );
        assert_eq!(mapa["n1"], vec![texto(" texto")]);
    }
}
