//! Propriedades de run (`w:rPr`): negrito, itálico, tachado e fonte monoespaçada.
//! Só interpreta as propriedades; quem monta o texto dos runs fica no módulo de parágrafo.

use crate::contexto::Contexto;
use crate::modelo::Marcas;
use crate::xml_leitor::{Evento, Leitor};

/// Fontes monoespaçadas que contam como código; o casamento ignora maiúsculas.
/// Lista curta de propósito: as fontes do Word que não estão aqui continuam como texto comum.
const FONTES_MONO: [&str; 9] = [
    "courier new",
    "courier",
    "consolas",
    "lucida console",
    "menlo",
    "monaco",
    "source code pro",
    "liberation mono",
    "dejavu sans mono",
];

/// Lê um `w:rPr` (chamada logo depois do `Abre("rPr")`) e consome até o `Fecha` dele.
/// Propriedade direta vence o estilo: `<w:b w:val="0"/>` desliga o negrito mesmo que o estilo
/// de caractere o ligue. Por isso cada marca fica `Option` até o fim e só então cai no estilo.
pub(crate) fn ler_rpr(l: &mut Leitor, ctx: &Contexto) -> Result<Marcas, String> {
    let mut negrito = None;
    let mut italico = None;
    let mut tachado = None;
    let mut codigo = None;
    let mut estilo_id = None;
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "rPr" => break,
            Some(Evento::Abre(tag)) => {
                match tag.nome.as_str() {
                    // `rStyle` só guarda o id: o estilo é consultado no fim, junto das diretas.
                    "rStyle" => estilo_id = tag.val().map(str::to_string),
                    "b" => negrito = Some(tag.ligado()),
                    "i" => italico = Some(tag.ligado()),
                    "strike" | "dstrike" => tachado = Some(tag.ligado()),
                    // Com `ascii` ou `hAnsi` presente, a fonte decide o código, mesmo se não for mono.
                    "rFonts" => {
                        let nomes = [tag.attr("ascii"), tag.attr("hAnsi")];
                        if nomes.iter().any(Option::is_some) {
                            codigo = Some(nomes.into_iter().flatten().any(fonte_mono));
                        }
                    }
                    _ => {}
                }
                l.pular()?;
            }
            Some(_) => {}
        }
    }
    let estilo = estilo_id.map(|id| ctx.estilos.de(&id)).unwrap_or_default();
    Ok(Marcas {
        negrito: negrito.unwrap_or(estilo.negrito),
        italico: italico.unwrap_or(estilo.italico),
        tachado: tachado.unwrap_or(false),
        codigo: codigo.unwrap_or(estilo.codigo),
    })
}

/// Diz se o nome de fonte está na lista de monoespaçadas, sem diferenciar maiúsculas.
fn fonte_mono(nome: &str) -> bool {
    let minusculo = nome.to_lowercase();
    FONTES_MONO.contains(&minusculo.as_str())
}

#[cfg(test)]
mod tests {
    use crate::contexto::com_contexto;
    use crate::xml_leitor::Leitor;
    use crate::Unlimited;

    /// Lê um `w:rPr` com estes filhos dentro do contexto de teste.
    fn marcas(corpo: &str, estilos: &str) -> crate::modelo::Marcas {
        let xml = format!(r#"<w:rPr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{corpo}</w:rPr>"#);
        com_contexto(estilos, "", |ctx| {
            let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
            l.ate("rPr").unwrap().unwrap();
            crate::estilo_run::ler_rpr(&mut l, ctx).unwrap()
        })
    }

    #[test]
    fn negrito_italico_tachado() {
        let m = marcas("<w:b/><w:i/><w:strike/>", "");
        assert!(m.negrito && m.italico && m.tachado);
        assert!(!m.codigo);
    }

    #[test]
    fn val_zero_desliga() {
        assert!(!marcas(r#"<w:b w:val="0"/>"#, "").negrito);
    }

    #[test]
    fn dstrike_conta_como_tachado() {
        assert!(marcas("<w:dstrike/>", "").tachado);
    }

    #[test]
    fn fonte_mono_vira_codigo() {
        assert!(marcas(r#"<w:rFonts w:ascii="Courier New"/>"#, "").codigo);
        assert!(marcas(r#"<w:rFonts w:ascii="consolas"/>"#, "").codigo);
        assert!(!marcas(r#"<w:rFonts w:ascii="Calibri"/>"#, "").codigo);
    }

    #[test]
    fn estilo_de_caractere_strong() {
        let estilos = r#"<w:style w:styleId="Forte"><w:name w:val="strong"/></w:style>"#;
        let m = marcas(r#"<w:rStyle w:val="Forte"/>"#, estilos);
        assert!(m.negrito);
    }

    #[test]
    fn propriedade_direta_vence_o_estilo() {
        let estilos = r#"<w:style w:styleId="Forte"><w:name w:val="strong"/></w:style>"#;
        let m = marcas(r#"<w:rStyle w:val="Forte"/><w:b w:val="0"/>"#, estilos);
        assert!(!m.negrito);
    }

    #[test]
    fn rpr_vazio_da_marcas_padrao() {
        assert_eq!(marcas("", ""), crate::modelo::Marcas::default());
    }
}
