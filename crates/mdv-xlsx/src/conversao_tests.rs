use crate::testutil::XlsxBuilder;
use crate::{converter, to_markdown, Limites, Unlimited};

#[test]
fn converte_planilha_com_formula_e_aba_oculta() {
    let bytes = XlsxBuilder::novo()
        .textos(&["Produto", "Qtd", "Caneta", "Total"])
        .aba("Vendas", r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
            <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>3</v></c></row>
            <row r="3"><c r="A3" t="s"><v>3</v></c><c r="B3"><f>SUM(B2:B2)</f><v>3</v></c></row>"#)
        .aba_oculta("Rascunho", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)
        .construir();
    let md = to_markdown(&bytes, &Unlimited).unwrap();
    assert_eq!(md, "# Vendas\n\n| Produto | Qtd |\n| --- | ---: |\n| Caneta | 3 |\n| Total | 3 |\n\n");
    assert!(!md.contains("SUM") && !md.contains("Rascunho"));
}

#[test]
fn so_abas_ocultas_ou_nenhuma_aba_da_aviso() {
    let bytes = XlsxBuilder::novo().aba_oculta("X", "").construir();
    assert_eq!(to_markdown(&bytes, &Unlimited).unwrap(), "_(a planilha não tem abas visíveis)_\n");
}

#[test]
fn teto_de_abas_corta_com_aviso() {
    let l = Limites { max_abas: 1, ..Limites::default() };
    let bytes = XlsxBuilder::novo().aba("A", "").aba("B", "").construir();
    assert!(converter(&bytes, &Unlimited, &l).unwrap().contains("_(planilha cortada: 1 de 2 abas mostradas)_"));
}

#[test]
fn arquivos_que_nao_sao_xlsx_dao_erro_em_portugues() {
    let ole2 = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0];
    assert!(to_markdown(&ole2, &Unlimited).unwrap_err().contains("antigo"));
    assert!(to_markdown(b"texto qualquer", &Unlimited).unwrap_err().contains("ZIP inválido"));
}

#[test]
fn titulo_com_hash_final_nao_vira_fechamento_de_cabecalho() {
    let bytes = XlsxBuilder::novo().aba("Vendas #", "").construir();
    let md = to_markdown(&bytes, &Unlimited).unwrap();
    assert!(md.starts_with("# Vendas \\#\n"), "{md}");
}

#[test]
fn titulo_com_hash_e_espaco_final_nao_vira_fechamento_de_cabecalho() {
    let bytes = XlsxBuilder::novo().aba("Vendas # ", "").construir();
    let md = to_markdown(&bytes, &Unlimited).unwrap();
    assert!(md.starts_with("# Vendas \\#\n"), "{md}");
}

#[test]
fn aba_cuja_parte_nao_existe_no_pacote_tem_aviso_proprio() {
    // Pasta que aponta `Sumida` para uma parte que não está no ZIP: não é aba vazia.
    let ns_sheet = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let ns_rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let ns_pkg = "http://schemas.openxmlformats.org/package/2006/relationships";
    let raiz = format!(r#"<Relationships xmlns="{ns_pkg}"><Relationship Id="rId1" Type="{ns_rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#);
    let wb = format!(r#"<workbook xmlns="{ns_sheet}" xmlns:r="{ns_rel}"><sheets><sheet name="Sumida" sheetId="1" r:id="rId1"/></sheets></workbook>"#);
    let rels = format!(r#"<Relationships xmlns="{ns_pkg}"><Relationship Id="rId1" Type="{ns_rel}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#);
    let bytes = crate::testutil::zip(
        &[("_rels/.rels", raiz.as_bytes()), ("xl/workbook.xml", wb.as_bytes()), ("xl/_rels/workbook.xml.rels", rels.as_bytes())],
        true,
    );
    let md = to_markdown(&bytes, &Unlimited).unwrap();
    assert_eq!(md, "# Sumida\n\n_(aba não encontrada no arquivo)_\n\n", "{md}");
}
