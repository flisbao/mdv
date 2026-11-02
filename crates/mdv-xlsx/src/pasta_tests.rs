use crate::pasta::{abrir, abrir_com_teto};
use crate::pacote::Pacote;
use crate::testutil::{zip, XlsxBuilder};
use crate::Unlimited;

#[test]
fn acha_as_abas_na_ordem_do_workbook_e_marca_as_ocultas() {
    let bytes = XlsxBuilder::novo()
        .aba("Vendas", "").aba_oculta("Segredo", "").aba("Resumo", "").construir();
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    let pasta = abrir(&mut pacote, &Unlimited).unwrap();
    let nomes: Vec<_> = pasta.abas.iter().map(|a| (a.nome.as_str(), a.oculta)).collect();
    assert_eq!(nomes, [("Vendas", false), ("Segredo", true), ("Resumo", false)]);
    assert_eq!(pasta.abas[2].parte, "xl/worksheets/sheet3.xml");
    assert!(!pasta.data_1904);
}

#[test]
fn le_data_1904_e_os_nomes_das_partes_de_textos_e_estilos() {
    let bytes = XlsxBuilder::novo().aba("A", "").textos(&["x"]).estilos("<cellXfs/>").data_1904().construir();
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    let pasta = abrir(&mut pacote, &Unlimited).unwrap();
    assert!(pasta.data_1904);
    assert_eq!(pasta.textos.as_deref(), Some("xl/sharedStrings.xml"));
    assert_eq!(pasta.estilos.as_deref(), Some("xl/styles.xml"));
}

#[test]
fn nome_de_aba_passa_por_sanitize() {
    let bytes = XlsxBuilder::novo().aba("A\u{1b}B", "").construir();
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    assert_eq!(abrir(&mut pacote, &Unlimited).unwrap().abas[0].nome, "A\u{fffd}B");
}

#[test]
fn zip_sem_workbook_e_xlsb_dao_erro_claro() {
    let bytes = zip(&[("xl/workbook.bin", b"x")], false);
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    let erro = abrir(&mut pacote, &Unlimited).unwrap_err();
    assert!(erro.contains("xl/workbook.xml"), "{erro}");
}

#[test]
fn nome_de_parte_com_controle_nao_vai_cru_para_o_erro() {
    // A raiz aponta o officeDocument para uma parte que não existe, com ESC no nome.
    let rels = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/\u{1b}[2Jw.xml\"/></Relationships>";
    let bytes = zip(&[("_rels/.rels", rels.as_bytes())], false);
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    let erro = abrir(&mut pacote, &Unlimited).unwrap_err();
    assert!(!erro.contains('\u{1b}'), "{erro:?}");
    assert!(erro.contains("não tem"), "{erro}");
}

#[test]
fn abas_demais_dao_erro_claro() {
    let bytes = XlsxBuilder::novo().aba("A", "").aba("B", "").aba("C", "").construir();
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    let erro = abrir_com_teto(&mut pacote, &Unlimited, 2).unwrap_err();
    assert!(erro.contains("abas demais"), "{erro}");
}

#[test]
fn exatamente_no_teto_de_abas_passa() {
    let bytes = XlsxBuilder::novo().aba("A", "").aba("B", "").aba("C", "").construir();
    let mut pacote = Pacote::abrir(&bytes).unwrap();
    assert_eq!(abrir_com_teto(&mut pacote, &Unlimited, 3).unwrap().abas.len(), 3);
}
