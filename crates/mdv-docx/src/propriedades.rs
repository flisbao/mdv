//! Propriedades de parágrafo (`w:pPr`): estilo, numeração direta e nível de outline.
//! Só lê o que o parágrafo declara; quem decide o que isso vira fica em `classificar`.

use crate::xml_leitor::{Evento, Leitor};

/// O que o `pPr` de um parágrafo declara diretamente (sem consultar estilos).
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Propriedades {
    /// `styleId` do `pStyle`.
    pub estilo: Option<String>,
    /// `(numId, ilvl)` do `numPr` direto; `numId` 0 é mantido, pois cancela a lista do estilo.
    pub num: Option<(u32, u8)>,
    /// `outlineLvl + 1` limitado a 6; `outlineLvl` 9 (texto comum) ou inválido fica `None`.
    pub nivel: Option<u8>,
    /// `outlineLvl` 9 declarado: o parágrafo é texto comum mesmo que o estilo seja um título.
    pub texto_comum: bool,
}

/// Lê um `w:pPr` (chamada logo depois do `Abre("pPr")`) e consome até o `Fecha` dele.
/// Só os filhos diretos contam; `rPr` (marca de parágrafo) e `pPrChange` (versão antiga) são pulados.
pub(crate) fn ler_ppr(l: &mut Leitor) -> Result<Propriedades, String> {
    let mut props = Propriedades::default();
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "pPr" => break,
            Some(Evento::Abre(tag)) => match tag.nome.as_str() {
                "pStyle" => {
                    props.estilo = tag.val().map(str::to_string);
                    l.pular()?;
                }
                "outlineLvl" => {
                    props.texto_comum = tag.val() == Some("9");
                    props.nivel = tag
                        .val()
                        .and_then(|v| v.parse::<u8>().ok())
                        .filter(|&v| v <= 8)
                        .map(|v| (v + 1).min(6));
                    l.pular()?;
                }
                "numPr" => props.num = ler_num_pr(l)?,
                _ => l.pular()?,
            },
            Some(_) => {}
        }
    }
    Ok(props)
}

/// Lê o `numPr` direto: sem `numId` não há lista (fica a do estilo); `ilvl` ausente é o nível 0.
fn ler_num_pr(l: &mut Leitor) -> Result<Option<(u32, u8)>, String> {
    let mut num_id = None;
    let mut ilvl = 0u8;
    loop {
        match l.proximo()? {
            None => return Err("fim inesperado do documento".into()),
            Some(Evento::Fecha(n)) if n == "numPr" => break,
            Some(Evento::Abre(tag)) => {
                match tag.nome.as_str() {
                    "numId" => num_id = tag.val().and_then(|v| v.parse::<u32>().ok()),
                    // Valor enorme satura em vez de zerar; `classificar` limita ao teto de listas.
                    "ilvl" => {
                        ilvl = tag
                            .val()
                            .and_then(|v| v.parse::<u32>().ok())
                            .map_or(0, |v| u8::try_from(v).unwrap_or(u8::MAX));
                    }
                    _ => {}
                }
                l.pular()?;
            }
            Some(_) => {}
        }
    }
    Ok(num_id.map(|n| (n, ilvl)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::NS;
    use crate::Unlimited;

    fn ler(corpo: &str) -> Propriedades {
        let xml = format!("<w:pPr {NS}>{corpo}</w:pPr>");
        let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
        l.ate("pPr").unwrap().unwrap();
        let props = ler_ppr(&mut l).unwrap();
        assert_eq!(l.proximo().unwrap(), None, "o pPr deve ser consumido por inteiro");
        props
    }

    #[test]
    fn le_estilo_numeracao_e_outline() {
        let p = ler(
            r#"<w:pStyle w:val="X"/><w:numPr><w:ilvl w:val="1"/><w:numId w:val="4"/></w:numPr><w:outlineLvl w:val="1"/>"#,
        );
        assert_eq!(p.estilo.as_deref(), Some("X"));
        assert_eq!(p.num, Some((4, 1)));
        assert_eq!(p.nivel, Some(2));
    }

    #[test]
    fn outline_9_nao_e_titulo() {
        let p = ler(r#"<w:outlineLvl w:val="9"/>"#);
        assert_eq!(p.nivel, None);
        assert!(p.texto_comum);
    }

    #[test]
    fn outline_alto_limita_a_6() {
        assert_eq!(ler(r#"<w:outlineLvl w:val="8"/>"#).nivel, Some(6));
    }

    #[test]
    fn rpr_dentro_do_ppr_e_ignorado() {
        let p = ler(r#"<w:rPr><w:pStyle w:val="Falso"/><w:outlineLvl w:val="0"/></w:rPr>"#);
        assert_eq!(p, Propriedades::default());
    }

    #[test]
    fn ppr_vazio() {
        assert_eq!(ler(""), Propriedades::default());
    }

    #[test]
    fn num_id_zero_e_mantido_e_ilvl_ausente_e_zero() {
        assert_eq!(ler(r#"<w:numPr><w:numId w:val="0"/></w:numPr>"#).num, Some((0, 0)));
    }

    #[test]
    fn num_pr_sem_num_id_nao_declara_lista() {
        assert_eq!(ler(r#"<w:numPr><w:ilvl w:val="2"/></w:numPr>"#).num, None);
    }

    #[test]
    fn ppr_change_com_estilo_antigo_e_ignorado() {
        let p = ler(r#"<w:pPrChange><w:pPr><w:pStyle w:val="Velho"/></w:pPr></w:pPrChange>"#);
        assert_eq!(p, Propriedades::default());
    }
}
