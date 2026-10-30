//! Testes de `ler_tabela`: linhas, células, mesclagens, tabelas aninhadas e elementos ignorados.

use crate::contexto::com_contexto;
use crate::modelo::{Celula, Marcas, Tabela, Trecho};
use crate::tabela::ler_xml;
use crate::MAX_COLUNAS;

fn tabela(corpo: &str) -> Tabela {
    com_contexto("", "", |ctx| ler_xml(ctx, corpo))
}

fn texto(s: &str) -> Trecho {
    Trecho::Texto { texto: s.into(), marcas: Marcas::default(), link: None }
}

fn celula(props: &str, s: &str) -> String {
    format!("<w:tc>{props}<w:p><w:r><w:t>{s}</w:t></w:r></w:p></w:tc>")
}

fn uma_celula(props: &str) -> Celula {
    let t = tabela(&format!("<w:tr>{}</w:tr>", celula(props, "x")));
    t.linhas.into_iter().next().unwrap().into_iter().next().unwrap()
}

#[test]
fn linhas_e_celulas() {
    let corpo = format!(
        "<w:tr>{}{}</w:tr><w:tr>{}{}</w:tr>",
        celula("", "a"), celula("", "b"), celula("", "c"), celula("", "d")
    );
    let t = tabela(&corpo);
    assert_eq!(t.linhas.len(), 2);
    assert!(t.linhas.iter().all(|l| l.len() == 2));
    assert_eq!(t.linhas[0][1].paragrafos, vec![vec![texto("b")]]);
}

#[test]
fn gridspan_vira_colunas() {
    assert_eq!(uma_celula(r#"<w:tcPr><w:gridSpan w:val="2"/></w:tcPr>"#).colunas, 2);
    assert_eq!(uma_celula("").colunas, 1);
    assert_eq!(uma_celula("<w:tcPr><w:tcW w:w=\"10\"/></w:tcPr>").colunas, 1);
}

#[test]
fn gridspan_invalido_vale_um() {
    assert_eq!(uma_celula(r#"<w:tcPr><w:gridSpan w:val="abc"/></w:tcPr>"#).colunas, 1);
    assert_eq!(uma_celula(r#"<w:tcPr><w:gridSpan w:val="0"/></w:tcPr>"#).colunas, 1);
    assert_eq!(uma_celula(r#"<w:tcPr><w:gridSpan w:val="-3"/></w:tcPr>"#).colunas, 1);
    assert_eq!(uma_celula(r#"<w:tcPr><w:gridSpan/></w:tcPr>"#).colunas, 1);
}

#[test]
fn gridspan_enorme_e_limitado() {
    let c = uma_celula(r#"<w:tcPr><w:gridSpan w:val="100000"/></w:tcPr>"#);
    assert!(c.colunas <= MAX_COLUNAS);
    assert_eq!(c.colunas, MAX_COLUNAS);
    let c = uma_celula(r#"<w:tcPr><w:gridSpan w:val="99999999999999999999"/></w:tcPr>"#);
    assert_eq!(c.colunas, 1, "número que não cabe no inteiro é inválido");
}

#[test]
fn vmerge_continue_marca_continua() {
    assert!(uma_celula(r#"<w:tcPr><w:vMerge/></w:tcPr>"#).continua);
    assert!(uma_celula(r#"<w:tcPr><w:vMerge w:val="continue"/></w:tcPr>"#).continua);
    assert!(!uma_celula(r#"<w:tcPr><w:vMerge w:val="restart"/></w:tcPr>"#).continua);
    assert!(!uma_celula("").continua);
}

#[test]
fn celula_com_dois_paragrafos() {
    let t = tabela("<w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr>");
    assert_eq!(t.linhas[0][0].paragrafos, vec![vec![texto("a")], vec![texto("b")]]);
}

#[test]
fn paragrafo_vazio_na_celula_some() {
    let t = tabela("<w:tr><w:tc><w:p/><w:p><w:r><w:t>a</w:t></w:r></w:p><w:p/></w:tc></w:tr>");
    assert_eq!(t.linhas[0][0].paragrafos, vec![vec![texto("a")]]);
    let t = tabela("<w:tr><w:tc><w:p/></w:tc></w:tr>");
    assert!(t.linhas[0][0].paragrafos.is_empty());
}

#[test]
fn tabela_aninhada_e_achatada_na_celula() {
    let interna = format!("<w:tbl><w:tr>{}{}</w:tr></w:tbl>", celula("", "i1"), celula("", "i2"));
    let corpo = format!(
        "<w:tr><w:tc><w:p><w:r><w:t>antes</w:t></w:r></w:p>{interna}<w:p><w:r><w:t>depois</w:t></w:r></w:p></w:tc>{}</w:tr>",
        celula("", "irma")
    );
    let t = tabela(&corpo);
    assert_eq!(t.linhas.len(), 1);
    assert_eq!(t.linhas[0].len(), 2);
    assert_eq!(
        t.linhas[0][0].paragrafos,
        vec![vec![texto("antes")], vec![texto("i1")], vec![texto("i2")], vec![texto("depois")]]
    );
}

#[test]
fn tabela_sem_linhas() {
    assert!(tabela("<w:tblPr/><w:tblGrid/>").linhas.is_empty());
}

#[test]
fn tblpr_trpr_e_tblgrid_sao_ignorados() {
    let corpo = format!(
        r#"<w:tblPr><w:tblStyle w:val="x"/><w:tc><w:p><w:r><w:t>nao</w:t></w:r></w:p></w:tc></w:tblPr>
        <w:tblGrid><w:gridCol w:w="100"/></w:tblGrid>
        <w:tr><w:trPr><w:tc><w:p><w:r><w:t>nao</w:t></w:r></w:p></w:tc></w:trPr><w:tblPrEx><w:tc/></w:tblPrEx>{}</w:tr>"#,
        celula("", "sim")
    );
    let t = tabela(&corpo);
    assert_eq!(t.linhas.len(), 1);
    assert_eq!(t.linhas[0].len(), 1);
    assert_eq!(t.linhas[0][0].paragrafos, vec![vec![texto("sim")]]);
}

#[test]
fn linhas_com_numero_diferente_de_celulas() {
    let corpo = format!(
        "<w:tr>{}{}{}</w:tr><w:tr>{}</w:tr><w:tr/>",
        celula("", "a"), celula("", "b"), celula("", "c"), celula("", "d")
    );
    let t = tabela(&corpo);
    let tamanhos: Vec<usize> = t.linhas.iter().map(Vec::len).collect();
    assert_eq!(tamanhos, [3, 1, 0]);
}

#[test]
fn sdt_em_volta_da_celula_e_do_paragrafo_e_transparente() {
    let corpo = "<w:tr><w:sdt><w:sdtContent><w:tc><w:sdt><w:sdtContent><w:p><w:r><w:t>a</w:t></w:r></w:p></w:sdtContent></w:sdt></w:tc></w:sdtContent></w:sdt></w:tr>";
    let t = tabela(corpo);
    assert_eq!(t.linhas[0][0].paragrafos, vec![vec![texto("a")]]);
}

#[test]
fn tabela_truncada_e_erro_sem_panico() {
    use crate::contexto::com_contexto;
    use crate::tabela::ler_tabela;
    use crate::testutil::NS;
    use crate::xml_leitor::Leitor;
    use crate::Unlimited;
    let xml = format!("<w:tbl {NS}><w:tr><w:tc><w:p>");
    let resultado = com_contexto("", "", |ctx| {
        let mut l = Leitor::novo(xml.as_bytes(), &Unlimited);
        l.ate("tbl").unwrap().unwrap();
        ler_tabela(&mut l, ctx)
    });
    assert!(resultado.is_err());
}
