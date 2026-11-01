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
